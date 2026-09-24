use crate::gpu::BatchItem;
use crate::metrics::LocalMetrics;
use crate::table::{CompactRegretTable, StrategyOp};
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{Action, ActionKind, GameState, Street};
use rand::Rng;
use rand::RngExt;

const K: usize = 6;
const MAX_DEPTH: u32 = 50;

/// PKR_SKIP_FORCED=1 -> skip forced-move nodes (single legal bucket, e.g.
/// after an all-in call on a completed street). Default off. Changes RNG
/// streams when on, so not comparable bit-for-bit to a run where off.
fn skip_forced_nodes() -> bool {
    use std::sync::OnceLock;
    static S: OnceLock<bool> = OnceLock::new();
    *S.get_or_init(|| std::env::var("PKR_SKIP_FORCED").as_deref() == Ok("1"))
}

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
/// PKR_AVG_POWER=p -> strategy-sum weight t^p (0 = uniform, 1 = linear, 2 = DCFR gamma).
/// Default 0 reproduces the current behaviour exactly.
fn avg_weight_power() -> f32 {
    use std::sync::OnceLock;
    static P: OnceLock<f32> = OnceLock::new();
    *P.get_or_init(|| {
        std::env::var("PKR_AVG_POWER")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|p| (0.0..=4.0).contains(p))
            .unwrap_or(0.0)
    })
}

#[inline]
fn avg_weight(t: u32) -> f32 {
    let p = avg_weight_power();
    if p == 0.0 {
        1.0
    } else if p == 1.0 {
        t as f32
    } else if p == 2.0 {
        let x = t as f32;
        x * x
    } else {
        (t as f32).powf(p)
    }
}

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

/// Sample one action bucket from the ε-mixed distribution over LEGAL
/// buckets only. `strategy` is the regret-matched distribution,
/// already renormalized over legal buckets (`action_counts[a] > 0`).
///
/// The mixed probability of legal bucket `a` is:
///   p(a) = eps / n_legal + (1 - eps) * strategy[a]
/// which sums to `eps + (1-eps)*1 = 1` (mod float ULP).
///
/// Returns None iff no bucket is legal (caller must handle).
///
/// Design notes (r3 V2):
///   * Uses `r < cdf`, not `r <= acc`, so `r = 0.0` never picks a
///     zero-probability bucket.
///   * Float-underflow fallback is the LAST LEGAL bucket, never a
///     hardcoded `K-1`.
///   * Mixing is over legal buckets only; illegal buckets never receive
///     probability mass.
#[inline]
fn sample_bucket_epsilon(
    strategy: &[f32; K],
    action_counts: &[usize; K],
    eps: f32,
    r: f32,
) -> Option<usize> {
    // Renormalize defensively: the mixed distribution is defined over
    // the *normalized* regret-matched strategy restricted to legal
    // buckets. The real caller already renormalizes, so this is a no-op
    // in production — but if a future caller forgets, the sampler still
    // produces a valid distribution summing to 1 instead of falling
    // through to `last_legal` and biasing the tail bucket.
    let mut legal_total = 0.0f32;
    let mut n_legal = 0usize;
    for a in 0..K {
        if action_counts[a] > 0 {
            legal_total += strategy[a];
            n_legal += 1;
        }
    }
    if n_legal == 0 {
        return None;
    }
    let n_f = n_legal as f32;
    let inv_total = if legal_total > 0.0 {
        1.0 / legal_total
    } else {
        0.0
    };
    let uniform = 1.0 / n_f;

    let mut cdf = 0.0f32;
    let mut last_legal: Option<usize> = None;
    for a in 0..K {
        if action_counts[a] == 0 {
            continue;
        }
        last_legal = Some(a);
        let norm = if legal_total > 0.0 {
            strategy[a] * inv_total
        } else {
            uniform
        };
        let p = eps / n_f + (1.0 - eps) * norm;
        cdf += p;
        if r < cdf {
            return Some(a);
        }
    }
    // Float ULP: cdf may fall a hair short of 1.0. Returning the last
    // legal bucket is the correct closure.
    last_legal
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
        // C5d: count this silent-0.0 return so training aborts.
        metrics.depth_overflows += 1;
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
            // C5d: count silent deck exhaustion.
            metrics.deck_overflows += 1;
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
    // P1-f: bucket per concrete-action index. 8 bytes vs the old
    // 480-byte [[usize;10];K] table that was memset on every node.
    // `pick_in_bucket` (below) reconstructs the ordinal -> index map.
    let mut bucket_of_action = [0u8; 8];
    // C3: `action_bucket` always returns a bucket index for every
    // concrete action. Pre-action scalars match the traverser's
    // convention.
    let actor_stacks = current.stacks[current.actor];
    let actor_street = current.street_bets[current.actor];
    let actor_pot = current.pot;
    let opp_street = current.street_bets[1 - current.actor];
    for (idx, action) in num_actions.iter().enumerate() {
        let a = pkr_core::abstraction::action_bucket(
            &action.kind,
            actor_stacks,
            actor_street,
            opp_street,
            actor_pot,
        ) as usize;
        bucket_of_action[idx] = a as u8;
        action_counts[a] += 1;
    }
    // The ordinal-th (0-based) concrete action in `bucket`.
    let pick_in_bucket = |bucket: u8, ordinal: usize| -> usize {
        let mut seen = 0usize;
        for (i, &b) in bucket_of_action.iter().enumerate().take(num_actions_n) {
            if b == bucket {
                if seen == ordinal {
                    return i;
                }
                seen += 1;
            }
        }
        unreachable!("pick_in_bucket: bucket {bucket} ordinal {ordinal} out of range");
    };

    // E3: forced move -- only one legal bucket. No decision, no regret,
    // no strategy. Recurse without touching the table. Off by default
    // (PKR_SKIP_FORCED=1 to enable).
    if skip_forced_nodes() {
        let n_legal_buckets = action_counts.iter().filter(|&&c| c > 0).count();
        if n_legal_buckets == 1 {
            let a = (0..K).find(|&a| action_counts[a] > 0).unwrap();
            let count = action_counts[a];
            let ordinal = if count > 1 { rng.random_range(0..count) } else { 0 };
            current.apply_action_in_place(&num_actions[pick_in_bucket(a as u8, ordinal)]);
            let child_deck_idx = *deck_idx;
            let v = traverse(
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
            return v;
        }
    }

    // Compact history signature: (actions_this_street, num_raises,
    // last_was_bet). Collapses the infoset key space vs. hashing the
    // raw action sequence.
    let mut sig_buf = [0u8; 8];
    let sig_len = current.infoset_signature_into(&mut sig_buf);
    let history_bytes: &[u8] = &sig_buf[..sig_len];

    let hole = &current.hole[acting_player];
    // Hash on the valid community-card slice, not the padded [u8; 5] array.
    // The abstraction's match on board.len() distinguishes preflop(0),
    // flop(3), turn(4), river(5). Passing the raw array always sends
    // length 5, which routes every call through the river branch and
    // ignores the preflop/flop/turn tables entirely.
    let board: &[u8] = &current.board[..current.board_len as usize];
    let street_code = current.street as u8;
    let infoset_hash = abstraction.get_infoset_hash(hole, board, history_bytes, street_code);

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
        let w_avg = avg_weight(global_iteration);
        for a in 0..K {
            if strategy[a] <= 0.0 {
                continue;
            }
            strategy_batch.push(StrategyOp {
                index: idx as u32,
                action: a as u8,
                prob: strategy[a] * reach_prob * w_avg,
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
            let pick_idx = pick_in_bucket(a as u8, rng.random_range(0..count));

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
            if action_counts[a] == 0 || v[a].is_nan() {
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
        // Opponent node: sample one action from the ε-mixed distribution
        // over legal buckets. See `sample_bucket_epsilon` for the design
        // rationale and r3 V2 for the edge-case requirements.
        let eps = exploration_epsilon();
        let r = rng.random::<f32>();
        let sampled_abstract = match sample_bucket_epsilon(&strategy, &action_counts, eps, r) {
            Some(a) => a,
            None => {
                if advanced {
                    current.undo_action();
                }
                *deck_idx = saved_deck_idx;
                return 0.0;
            }
        };
        let count = action_counts[sampled_abstract];
        if count == 0 {
            if advanced {
                current.undo_action();
            }
            *deck_idx = saved_deck_idx;
            return 0.0;
        }
        let pick_idx = pick_in_bucket(sampled_abstract as u8, rng.random_range(0..count));

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

// (C3) local `abstract_action_index` deleted; use `pkr_core::abstraction::action_bucket`.

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

    // -----------------------------------------------------------------
    // V2: ε-uniform opponent sampling — legal buckets, no zero-prob pick.
    // -----------------------------------------------------------------

    #[test]
    fn sample_epsilon_empty_legal_set_returns_none() {
        let strategy = [1.0f32; K];
        let counts = [0usize; K];
        assert!(sample_bucket_epsilon(&strategy, &counts, 0.05, 0.5).is_none());
    }

    #[test]
    fn sample_epsilon_only_returns_legal_buckets() {
        // Strategy mass lives only on bucket 3, but buckets 3 and 5 are
        // legal. Every r in [0, 1) must map to 3 or 5.
        let mut strategy = [0.0f32; K];
        strategy[3] = 1.0;
        let mut counts = [0usize; K];
        counts[3] = 1;
        counts[5] = 1;

        let mut hit = [0usize; K];
        let n = 10_000;
        for i in 0..n {
            let r = i as f32 / n as f32;
            let a =
                sample_bucket_epsilon(&strategy, &counts, 0.05, r).expect("legal buckets exist");
            assert!(
                counts[a] > 0,
                "sampler returned illegal bucket {a} at r={r}"
            );
            hit[a] += 1;
        }
        // eps=0.05, 2 legal buckets, strategy concentrated on bucket 3:
        //   P(3) = 0.05/2 + 0.95*1.0 = 0.975
        //   P(5) = 0.05/2 + 0.95*0.0 = 0.025
        let p3 = hit[3] as f32 / n as f32;
        let p5 = hit[5] as f32 / n as f32;
        assert!(
            (p3 - 0.975).abs() < 0.005,
            "P(bucket 3) = {p3}, expected 0.975"
        );
        assert!(
            (p5 - 0.025).abs() < 0.005,
            "P(bucket 5) = {p5}, expected 0.025"
        );
    }

    #[test]
    fn sample_epsilon_r_zero_does_not_pick_zero_prob_bucket() {
        // Strategy mass on bucket 3; buckets 0 and 3 legal.
        // r = 0.0: must return bucket 0 with probability eps/2 = 0.025,
        // NOT bucket 3 just because r is the smallest value.
        let mut strategy = [0.0f32; K];
        strategy[3] = 1.0;
        let mut counts = [0usize; K];
        counts[0] = 1;
        counts[3] = 1;

        let a0 = sample_bucket_epsilon(&strategy, &counts, 0.05, 0.0).unwrap();
        assert_eq!(
            a0, 0,
            "r=0.0 must pick the first legal bucket (0), got {a0}"
        );
    }

    #[test]
    fn sample_epsilon_matches_expected_distribution_over_many_draws() {
        // Three legal buckets {1, 2, 4} with UN-normalized strategy mass
        // [0.5, 0.25, 0.0]. The sampler renormalizes defensively, so the
        // effective regret-matched distribution on {1,2,4} is
        // [0.5/0.75, 0.25/0.75, 0.0] = [0.6667, 0.3333, 0.0].
        //
        // Expected:
        //   P(1) = eps/3 + (1-eps) * 0.6667 = 0.6500
        //   P(2) = eps/3 + (1-eps) * 0.3333 = 0.3333
        //   P(4) = eps/3 + (1-eps) * 0.0    = 0.0167   (pure exploration)
        let mut strategy = [0.0f32; K];
        strategy[1] = 0.5;
        strategy[2] = 0.25;
        // strategy[4] stays 0.0
        let mut counts = [0usize; K];
        counts[1] = 1;
        counts[2] = 1;
        counts[4] = 1;

        let eps = 0.05f32;
        let n = 20_000;
        let mut hit = [0usize; K];
        for i in 0..n {
            let r = i as f32 / n as f32;
            let a = sample_bucket_epsilon(&strategy, &counts, eps, r).unwrap();
            assert!(counts[a] > 0);
            hit[a] += 1;
        }
        let exp1 = eps / 3.0 + (1.0 - eps) * (0.5 / 0.75);
        let exp2 = eps / 3.0 + (1.0 - eps) * (0.25 / 0.75);
        let exp4 = eps / 3.0;
        for (a, exp) in [(1usize, exp1), (2, exp2), (4, exp4)] {
            let got = hit[a] as f32 / n as f32;
            assert!(
                (got - exp).abs() < 0.01,
                "bucket {a}: got {got}, expected {exp}"
            );
        }
        // Buckets 0, 3, 5 must never appear.
        assert_eq!(hit[0], 0);
        assert_eq!(hit[3], 0);
        assert_eq!(hit[5], 0);
    }


    // ---- E3: forced-move node helper ----
    #[test]
    fn skip_forced_nodes_reads_env_off_by_default() {
        // Note: skip_forced_nodes() uses a OnceLock; this test only
        // asserts the default is "off" when the env var is not set.
        // The env var is not modified here because OnceLock caches on
        // first call and would poison later tests in this binary.
        if std::env::var("PKR_SKIP_FORCED").is_ok() {
            eprintln!("SKIP: PKR_SKIP_FORCED is set in the test env");
            return;
        }
        // Fresh process default: off.
        // (OnceLock means we can't call this twice with different env.)
        assert!(!skip_forced_nodes(), "default must be false");
    }

}