use crate::gpu::BatchItem;
use crate::metrics::LocalMetrics;
use crate::table::{CompactRegretTable, StrategyOp};
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{Action, ActionKind, GameState, Street};
use rand::Rng;
use rand::RngExt;

const K: usize = 6;
const MAX_DEPTH: u32 = 50;

/// FBRS (Brown & Sandholm, NeurIPS 2015) pruning.
///   - Warmup: don't prune before PRUNE_WARMUP iterations, so regrets have
///     time to accumulate signal.
///   - Threshold: prune when the action's regret is below -PRUNE_THRESHOLD
///     (fixed-point ×1000) AND regret matching gave it zero probability.
///   - 5% non-prune: keeps a small exploration tail so a truly recovering
///     action can re-enter measurement. The formal FBRS criterion is
///     r < -t * π_-i(I) * Δ; the absolute threshold is the conservative
///     common approximation.
const PRUNE_WARMUP: u32 = 1_000_000;
const PRUNE_THRESHOLD: i32 = -400_000; // -400 chips at SCALE=1000
const PRUNE_SKIP_PROB: f32 = 0.95;

/// Exploration floor at opponent nodes during MCCFR sampling.
///
/// Rationale: regret-matching+ clips negative regrets to 0, so an action
/// whose regret has been persistently negative gets probability 0. At
/// opponent nodes, MCCFR *samples* one action from the opponent's current
/// strategy, so a zero-probability action is never sampled. Any
/// downstream infoset the traverser would reach through that action then
/// receives no regret updates and its average strategy freezes at
/// whatever accumulated before the sampling collapsed.
///
/// Concretely: preflop, once BB learns that SB folds to jams, BB's
/// regret for jamming goes negative, jam is never sampled again, and SB's
/// facing-jam infoset (sig=0x00010102) freezes. This makes KK fold 95%
/// to a limp-rejam forever.
///
/// Epsilon-uniform exploration on top of regret-matching restores
/// reachability. EPSILON=0.05 keeps the sampled distribution close to the
/// intended strategy while ensuring every legal action has nonzero
/// probability. Override via PKR_EXPLORE_EPSILON for A/B testing.
fn exploration_epsilon() -> f32 {
    use std::sync::OnceLock;
    static E: OnceLock<f32> = OnceLock::new();
    *E.get_or_init(|| {
        std::env::var("PKR_EXPLORE_EPSILON")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|e| (0.0..1.0).contains(e))
            .unwrap_or(0.05)
    })
}

#[allow(clippy::too_many_arguments)]
pub fn traverse(
    current: &mut GameState,
    table: &CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    rng: &mut impl Rng,
    global_iteration: u32,
    traverser: usize,
    reach_prob: f32,
    deck: &[u8],
    deck_idx: &mut usize,
    depth: u32,
    batch: &mut Vec<BatchItem>,
    strategy_batch: &mut Vec<StrategyOp>,
    metrics: &mut LocalMetrics,
) -> f32 {
    metrics.record_node(depth);
    if depth > MAX_DEPTH {
        return 0.0;
    }

    let saved_deck_idx = *deck_idx;
    let advanced = if current.is_street_complete() && !current.is_terminal() {
        let cards_needed = match current.street {
            Street::Preflop => 3,
            Street::Flop => 1,
            Street::Turn => 1,
            Street::River => 0,
        };
        let start = *deck_idx;
        *deck_idx += cards_needed;
        debug_assert!(
            *deck_idx <= deck.len(),
            "runout deck exhausted: advanced to {} of {}",
            *deck_idx,
            deck.len(),
        );
        if *deck_idx > deck.len() {
            *deck_idx = saved_deck_idx;
            return 0.0;
        }
        let new_cards = &deck[start..*deck_idx];
        current.advance_street_in_place(new_cards);
        true
    } else {
        false
    };

    macro_rules! undo_advance_and_return {
        ($ret:expr) => {{
            if advanced {
                current.undo_action();
            }
            *deck_idx = saved_deck_idx;
            return $ret;
        }};
    }

    if current.is_terminal() {
        undo_advance_and_return!(current.terminal_payoff(traverser, evaluator));
    }

    let acting_player = current.actor;
    let mut action_buf: [Action; 8] = [Action {
        player: 0,
        kind: ActionKind::Fold,
    }; 8];
    let num_actions_n = current.legal_actions_into(&mut action_buf);
    if num_actions_n == 0 {
        undo_advance_and_return!(0.0);
    }
    let num_actions: &[Action] = &action_buf[..num_actions_n];

    let mut action_counts = [0usize; K];
    let mut action_indices = [[0usize; 10]; K];
    for (idx, action) in num_actions.iter().enumerate() {
        if let Some(a) = abstract_action_index(&action.kind, current) {
            if action_counts[a] < 10 {
                action_indices[a][action_counts[a]] = idx;
                action_counts[a] += 1;
            }
        }
    }

    // Compact history signature: (actions_this_street, num_raises,
    // last_was_bet). Collapses the infoset key space vs. hashing the
    // raw action sequence.
    let sig = current.history_signature();
    let history_bytes = sig.to_le_bytes();

    let hole = &current.hole[acting_player];
    // Hash on the valid community-card slice, not the padded [u8; 5] array.
    // The abstraction's match on board.len() distinguishes preflop(0),
    // flop(3), turn(4), river(5). Passing the raw array always sends
    // length 5, which routes every call through the river branch and
    // ignores the preflop/flop/turn tables entirely.
    let board: &[u8] = &current.board[..current.board_len as usize];
    let street_code = current.street as u8;
    let infoset_hash = abstraction.get_infoset_hash(hole, board, &history_bytes, street_code);

    let mut strategy = [0.0f32; K];
    let traverser_idx = if acting_player == traverser {
        Some(table.get_strategy_and_idx(infoset_hash, &mut strategy, metrics))
    } else {
        table.get_strategy_into(infoset_hash, &mut strategy);
        None
    };

    // --- Action masking: zero out buckets with no legal concrete
    // --- action, then renormalize over the legal ones.
    let legal_count = action_counts.iter().filter(|&&c| c > 0).count();
    if legal_count == 0 {
        undo_advance_and_return!(0.0);
    }
    let legal_total: f32 = (0..K)
        .filter(|&a| action_counts[a] > 0)
        .map(|a| strategy[a])
        .sum();
    if legal_total > 0.0 {
        for a in 0..K {
            if action_counts[a] == 0 {
                strategy[a] = 0.0;
            } else {
                strategy[a] /= legal_total;
            }
        }
    } else {
        let u = 1.0 / legal_count as f32;
        for a in 0..K {
            strategy[a] = if action_counts[a] > 0 { u } else { 0.0 };
        }
    }

    if let Some(idx) = traverser_idx {
        for a in 0..K {
            if strategy[a] <= 0.0 {
                continue;
            }
            strategy_batch.push(StrategyOp {
                index: idx as u32,
                action: a as u8,
                prob: strategy[a] * reach_prob,
            });
        }
    }

    if acting_player == traverser {
        let idx = traverser_idx.expect("traverser_idx set for traverser");
        let mut v = [0.0f32; K];
        for a in 0..K {
            let count = action_counts[a];
            if count == 0 {
                v[a] = f32::NAN;
                continue;
            }
            // FBRS pruning: skip a hopeless action (regret very negative,
            // probability already zero) most of the time. Sentinal value
            // is NaN, same as illegal buckets, so v_sigma and the push
            // loop already skip it.
            if global_iteration > PRUNE_WARMUP
                && strategy[a] == 0.0
                && table.regret_scaled(idx, a) < PRUNE_THRESHOLD
                && rng.random::<f32>() < PRUNE_SKIP_PROB
            {
                v[a] = f32::NAN;
                continue;
            }
            let pick_idx = action_indices[a][rng.random_range(0..count)];

            current.apply_action_in_place(&num_actions[pick_idx]);
            let child_deck_idx = *deck_idx;
            v[a] = traverse(
                current,
                table,
                abstraction,
                evaluator,
                rng,
                global_iteration,
                traverser,
                reach_prob * strategy[a],
                deck,
                &mut *deck_idx,
                depth + 1,
                batch,
                strategy_batch,
                metrics,
            );
            *deck_idx = child_deck_idx;
            current.undo_action();
        }

        let v_sigma: f32 = (0..K)
            .filter(|&a| !v[a].is_nan())
            .map(|a| strategy[a] * v[a])
            .sum();

        for a in 0..K {
            if action_counts[a] == 0 {
                continue;
            }
            let delta = v[a] - v_sigma;
            batch.push(BatchItem {
                index: idx as u32,
                action: a as u32,
                iteration: global_iteration,
                delta,
            });
        }

        if advanced {
            current.undo_action();
        }
        *deck_idx = saved_deck_idx;

        v_sigma
    } else {
        // Opponent node: sample one action. Mix in epsilon-uniform
        // exploration on top of regret-matching so actions with zero
        // regret-matching probability are still sampled occasionally.
        // Without this, an action whose regret has been persistently
        // negative (RM+ gives it 0) is never sampled, and any infoset
        // reachable only through that action freezes. See exploration_epsilon.
        let eps = exploration_epsilon();
        let mut sampled_abstract = usize::MAX;
        let r = rng.random::<f32>();
        if r < eps {
            // Uniform over legal buckets.
            let mut legal_count = 0usize;
            for a in 0..K {
                if action_counts[a] > 0 { legal_count += 1; }
            }
            if legal_count > 0 {
                let pick = rng.random_range(0..legal_count);
                let mut seen = 0usize;
                for a in 0..K {
                    if action_counts[a] > 0 {
                        if seen == pick { sampled_abstract = a; break; }
                        seen += 1;
                    }
                }
            }
        } else {
            // Regret-matched distribution, with the exploration mass
            // redistributed proportionally.
            let r2 = (r - eps) / (1.0 - eps);
            let mut acc = 0.0;
            for i in 0..K {
                if action_counts[i] == 0 { continue; }
                acc += strategy[i];
                if r2 <= acc {
                    sampled_abstract = i;
                    break;
                }
            }
        }
        // Fallback: uniform over legal buckets (covers r2 slightly past 1.0
        // due to float rounding, and empty-strategy cases).
        if sampled_abstract == usize::MAX || action_counts[sampled_abstract] == 0 {
            let legal: Vec<usize> = (0..K).filter(|&a| action_counts[a] > 0).collect();
            if legal.is_empty() {
                if advanced { current.undo_action(); }
                *deck_idx = saved_deck_idx;
                return 0.0;
            }
            sampled_abstract = legal[rng.random_range(0..legal.len())];
        }
        let count = action_counts[sampled_abstract];
        if count == 0 {
            if advanced {
                current.undo_action();
            }
            *deck_idx = saved_deck_idx;
            return 0.0;
        }
        let pick_idx = action_indices[sampled_abstract][rng.random_range(0..count)];

        current.apply_action_in_place(&num_actions[pick_idx]);
        let child_deck_idx = *deck_idx;
        let result = traverse(
            current,
            table,
            abstraction,
            evaluator,
            rng,
            global_iteration,
            traverser,
            reach_prob,
            deck,
            &mut *deck_idx,
            depth + 1,
            batch,
            strategy_batch,
            metrics,
        );
        *deck_idx = child_deck_idx;
        current.undo_action();

        if advanced {
            current.undo_action();
        }
        *deck_idx = saved_deck_idx;

        result
    }
}

fn abstract_action_index(kind: &ActionKind, state: &GameState) -> Option<usize> {
    match kind {
        ActionKind::Fold => Some(0),
        ActionKind::Check | ActionKind::Call => Some(1),
        ActionKind::Bet(amount) => {
            let pot = state.pot.max(1.2);
            let fraction = amount / pot;
            if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
                Some(5)
            } else if fraction < 0.6 {
                Some(2)
            } else if fraction < 1.2 {
                Some(3)
            } else {
                Some(4)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::CompactRegretTable;
    use pkr_abstraction::KMeansAbstraction;
    use rand::rngs::StdRng;
    use rand::seq::SliceRandom;
    use rand::SeedableRng;
    use std::sync::Arc;

    struct MockEvaluator;
    impl pkr_contracts::Evaluator for MockEvaluator {
        fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
            let mut sum: u32 = 0;
            for &c in hole {
                sum += c as u32;
            }
            for &c in board {
                sum += c as u32;
            }
            sum % 7462
        }
    }

    fn make_abstraction() -> Arc<KMeansAbstraction> {
        Arc::new(KMeansAbstraction::new(
            vec![(0.3, 0.09), (0.7, 0.49)],
            Arc::new(MockEvaluator),
        ))
    }

    #[test]
    fn traverse_does_not_cutoff_at_turn() {
        let table = Arc::new(CompactRegretTable::with_capacity(100_000));
        let abstraction = make_abstraction();
        let evaluator: Arc<dyn pkr_contracts::Evaluator> = Arc::new(MockEvaluator);

        let mut rng = StdRng::seed_from_u64(99);
        let mut batch = Vec::new();
        let mut strategy_batch = Vec::new();
        let mut metrics = LocalMetrics::default();
        let deck: Vec<u8> = (0..52).collect();

        let mut state = GameState::new(200.0, 1.0, 2.0);
        state.set_hole_cards([deck[0], deck[1]], [deck[2], deck[3]]);
        let deck_slice = &deck[4..];
        let mut deck_idx = 0usize;

        let result = traverse(
            &mut state,
            &table,
            abstraction.as_ref(),
            &*evaluator,
            &mut rng,
            1,
            0,
            1.0,
            deck_slice,
            &mut deck_idx,
            0,
            &mut batch,
            &mut strategy_batch,
            &mut metrics,
        );

        assert!(result.is_finite());
        assert!(result.abs() <= 200.0);
        assert!(metrics.nodes > 0);
    }

    #[test]
    fn turn_and_river_infosets_receive_strategy_sum_after_training() {
        let table = Arc::new(CompactRegretTable::with_capacity(100_000));
        let abstraction = make_abstraction();
        let evaluator: Arc<dyn pkr_contracts::Evaluator> = Arc::new(MockEvaluator);

        for iteration in 1..=60u32 {
            let mut rng = StdRng::seed_from_u64(1000 + iteration as u64);
            let mut batch = Vec::with_capacity(10000);
            let mut strategy_batch = Vec::with_capacity(10000);
            let mut metrics = LocalMetrics::default();
            let mut deck: Vec<u8> = (0..52).collect();
            deck.shuffle(&mut rng);

            let mut state = GameState::new(200.0, 1.0, 2.0);
            state.set_hole_cards([deck[0], deck[1]], [deck[2], deck[3]]);
            let deck_slice = &deck[4..];
            let mut deck_idx = 0usize;
            traverse(
                &mut state,
                &table,
                abstraction.as_ref(),
                &*evaluator,
                &mut rng,
                iteration,
                0,
                1.0,
                deck_slice,
                &mut deck_idx,
                0,
                &mut batch,
                &mut strategy_batch,
                &mut metrics,
            );

            let mut state2 = GameState::new(200.0, 1.0, 2.0);
            state2.set_hole_cards([deck[0], deck[1]], [deck[2], deck[3]]);
            let mut deck_idx2 = 0usize;
            traverse(
                &mut state2,
                &table,
                abstraction.as_ref(),
                &*evaluator,
                &mut rng,
                iteration,
                1,
                1.0,
                deck_slice,
                &mut deck_idx2,
                0,
                &mut batch,
                &mut strategy_batch,
                &mut metrics,
            );

            for op in &strategy_batch {
                table.add_strategy_sum_at(op.index as usize, op.action as usize, op.prob);
            }
            table.flush_cpu_batch(&mut batch);
        }

        let keys = table.get_keys();
        assert!(keys.len() > 50, "only {} infosets registered", keys.len());

        let mut non_uniform_count = 0;
        for key in keys {
            let mut strategy = [0.0f32; K];
            table.get_average_strategy_into(key, &mut strategy);
            if strategy.iter().any(|&p| p > 1.0 / K as f32 + 0.01) {
                non_uniform_count += 1;
            }
        }
        assert!(non_uniform_count > 0);
    }
}
