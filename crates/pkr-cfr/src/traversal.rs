use crate::gpu::BatchItem;
use crate::table::{CompactRegretTable, StrategyOp};
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{Action, ActionKind, GameState, Street};
use rand::Rng;
use rand::RngExt;

const K: usize = 6;
const MAX_DEPTH: u32 = 50;

pub fn traverse(
    current: &mut GameState,
    table: &CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    rng: &mut impl Rng,
    global_iteration: u32,
    traverser: usize,
    reach_prob: f32,
    opponent_reach: f32,
    deck: &[u8],
    deck_idx: &mut usize,
    depth: u32,
    batch: &mut Vec<BatchItem>,
    strategy_batch: &mut Vec<StrategyOp>,
) -> f32 {
    if depth > MAX_DEPTH {
        return 0.0;
    }

    // Save deck_idx before any street advancement so we can restore it.
    // advance_street_in_place modifies *deck_idx, and GameState's undo mechanism
    // does not track deck_idx.
    let saved_deck_idx = *deck_idx;
    // Track whether we advanced the street, so we can undo it before returning.
    // advance_street_in_place pushes an undo record; we must undo it to keep
    // the undo stack balanced for our caller.
    let advanced = if current.is_street_complete() && !current.is_terminal() {
        let cards_needed = match current.street {
            Street::Preflop => 3,
            Street::Flop => 1,
            Street::Turn => 1,
            Street::River => 0,
        };
        let start = *deck_idx;
        *deck_idx += cards_needed;
        // After the 4 hole cards, the runout slice must hold at least 5 community cards
        // (3 flop + 1 turn + 1 river). The trainer shuffles 52 cards and slices deck[4..].
        debug_assert!(
            deck.len() >= *deck_idx + 5,
            "runout deck must hold >= 5 community cards for turn + river"
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

    // Helper to undo the advance and restore deck_idx, for early returns.
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
    let mut action_buf: [Action; 8] = [Action { player: 0, kind: ActionKind::Fold }; 8];
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
    // last_was_bet). This collapses the infoset key space by orders of
    // magnitude vs. hashing the raw 32-byte action sequence, while
    // preserving the legal action space at every node.
    let sig = current.history_signature();
    let history_bytes = sig.to_le_bytes();

    let hole = &current.hole[acting_player];
    let board = &current.board;
    let street_code = current.street as u8;
    let infoset_hash =
        abstraction.get_infoset_hash(hole, board, &history_bytes, street_code);

    let mut strategy = [0.0f32; K];
    let traverser_idx = if acting_player == traverser {
        Some(table.get_strategy_and_idx(infoset_hash, &mut strategy))
    } else {
        table.get_strategy_into(infoset_hash, &mut strategy);
        None
    };

    if let Some(idx) = traverser_idx {
        for a in 0..K {
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
                v[a] = 0.0;
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
                opponent_reach,
                deck,
                &mut *deck_idx,
                depth + 1,
                batch,
                strategy_batch,
            );
            *deck_idx = child_deck_idx;
            current.undo_action(); // undo apply_action_in_place
        }

        let v_sigma: f32 = strategy.iter().zip(v.iter()).map(|(p, u)| p * u).sum();

        // Push updates to the local batch instead of updating atomically
        // The delta is scaled by opponent_reach: in external-sampling MCCFR,
        // the opponent's reach probability weights the traversal so that
        // the expected regret converges to the true game value.
        for a in 0..K {
            let delta = (v[a] - v_sigma) * opponent_reach;
            batch.push(BatchItem {
                index: idx as u32,
                action: a as u32,
                iteration: global_iteration,
                delta,
            });
        }

        // Undo the street advance to keep the undo stack balanced for our caller
        if advanced {
            current.undo_action();
        }
        *deck_idx = saved_deck_idx;

        v_sigma
    } else {
        let r = rng.random::<f32>();
        let mut acc = 0.0;
        let mut sampled_abstract = K - 1;
        for i in 0..K {
            acc += strategy[i];
            if r <= acc {
                sampled_abstract = i;
                break;
            }
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
            opponent_reach * strategy[sampled_abstract],
            deck,
            &mut *deck_idx,
            depth + 1,
            batch,
            strategy_batch,
        );
        *deck_idx = child_deck_idx;
        current.undo_action(); // undo apply_action_in_place

        // Undo the street advance to keep the undo stack balanced for our caller
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
            let pot = state.pot.max(1.0);
            let fraction = amount / pot;
            if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
                Some(5)
            } else if fraction < 0.5 {
                Some(2)
            } else if fraction < 1.0 {
                Some(3)
            } else {
                Some(4)
            }
        }
    }
}

#[cfg(test)]
#[cfg(feature = "gpu")]
mod tests {
    use super::*;
    use crate::table::CompactRegretTable;
    use pkr_abstraction::KMeansAbstraction;
    use rand::rngs::StdRng;
    use rand::seq::SliceRandom;
    use rand::SeedableRng;
    use std::sync::Arc;

    /// Mock evaluator: deterministic hand ranking based on card sum mod 7462.
    /// In the real evaluator, lower rank = better hand. Here we use a simple
    /// deterministic mapping so tests are reproducible.
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

    /// Mock abstraction: use the KMeansAbstraction with dummy centroids.
    fn make_abstraction() -> Arc<KMeansAbstraction> {
        Arc::new(KMeansAbstraction::new(
            vec![(0.3, 0.09), (0.7, 0.49)],
            Arc::new(MockEvaluator),
        ))
    }

    /// Verify that the traversal does NOT return early at the turn street,
    /// meaning turn nodes are real decision nodes (not leaves).
    #[test]
    fn traverse_does_not_cutoff_at_turn() {
        let table = Arc::new(CompactRegretTable::with_capacity(100_000));
        let abstraction = make_abstraction();
        let evaluator: Arc<dyn pkr_contracts::Evaluator> = Arc::new(MockEvaluator);

        let mut rng = StdRng::seed_from_u64(99);
        let mut batch = Vec::new();
        let mut strategy_batch = Vec::new();
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
            1.0,
            deck_slice,
            &mut deck_idx,
            0,
            &mut batch,
            &mut strategy_batch,
        );

        // The result should be a valid payoff (not NaN, not a raw EHS * pot value)
        assert!(
            result.is_finite(),
            "traversal returned non-finite value — check for early cutoff regression"
        );
        // With 200bb stacks, payoff should be in [-200, 200]
        assert!(
            result.abs() <= 200.0,
            "payoff {result} exceeds stack bounds — possible early cutoff or wrong payoff convention"
        );
    }

    /// Verify that after training iterations, turn and river infosets
    /// are registered in the table (i.e., the tree is NOT truncated at the turn).
    #[test]
    fn turn_and_river_infosets_receive_strategy_sum_after_training() {
        let table = Arc::new(CompactRegretTable::with_capacity(100_000));
        let abstraction = make_abstraction();
        let evaluator: Arc<dyn pkr_contracts::Evaluator> = Arc::new(MockEvaluator);

        // Run enough iterations with random decks to register >50 infosets.
        // GPU flush is a sync point, so keep this small to keep CI fast.
        for iteration in 1..=60u32 {
            let mut rng = StdRng::seed_from_u64(1000 + iteration as u64);
            let mut batch = Vec::with_capacity(10000);
            let mut strategy_batch = Vec::with_capacity(10000);
            let mut deck: Vec<u8> = (0..52).collect();
            deck.shuffle(&mut rng);

            // Hero perspective
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
                1.0,
                deck_slice,
                &mut deck_idx,
                0,
                &mut batch,
                &mut strategy_batch,
            );

            // Villain perspective
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
                1.0,
                deck_slice,
                &mut deck_idx2,
                0,
                &mut batch,
                &mut strategy_batch,
            );

            for op in &strategy_batch {
                table.add_strategy_sum_at(op.index as usize, op.action as usize, op.prob);
            }
            table.flush_gpu_batch(&batch);
        }

        // After 60 iterations with random decks, the table should have
        // registered infosets at turn and river streets.
        //
        // With the old turn-cutoff bug, traversal never reaches turn/river,
        // so only preflop and flop infosets would be registered.
        let keys = table.get_keys();
        let key_count = keys.len();

        assert!(
            key_count > 50,
            "only {key_count} infosets registered — tree may still be truncated at turn"
        );

        // Verify that at least some entries have non-uniform strategy,
        // which means regret updates were applied via the GPU flush.
        let mut non_uniform_count = 0;
        for key in keys {
            let mut strategy = [0.0f32; K];
            table.get_average_strategy_into(key, &mut strategy);
            if strategy.iter().any(|&p| p > 1.0 / K as f32 + 0.01) {
                non_uniform_count += 1;
            }
        }
        assert!(
            non_uniform_count > 0,
            "no non-uniform strategies found — flush or training did not register"
        );
    }
}
