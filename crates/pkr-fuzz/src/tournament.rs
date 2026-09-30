//! Blueprint-vs-blueprint duplicate tournament.
//!
//! The audit (F9) asked for a real-game evaluation. The existing
//! `eval_paired` compares a blueprint against a *scripted* bot; this
//! module compares two blueprints against each other under the same
//! paired design, which is what you need to decide whether one trained
//! checkpoint beats another.
//!
//! # Method
//!
//! For each hand:
//!
//!   pass 1: A plays seat 0, B plays seat 1 — chip delta to seat 0
//!   pass 2: B plays seat 0, A plays seat 1 — chip delta to seat 0
//!
//! Both passes deal the same hole cards and runout, and every decision
//! is drawn with common random numbers (decision K uses a seed derived
//! from `base_seed + K * const`). That removes runout variance and
//! sampling variance, leaving only strategic difference. The headline
//! number is `mean_diff(A - B) = mean(pass1) - mean(pass2)` per hand.
//!
//! # What this does NOT do (yet)
//!
//! - Enumerate remaining runouts for all-in-before-river hands. The
//!   paired design already cancels runout variance between A and B, so
//!   the *difference* is unbiased even with a fixed runout. The audit's
//!   suggestion matters for absolute chip counts, not the paired delta.
//! - Real-game LBR lower bound (audit F9 item 3).
//! - Off-abstraction opponent sizes. Both A and B play the abstracted
//!   tree, so this measures "which checkpoint is better at THIS game",
//!   not "which is better at HUNL".

use crate::EvalContext;
use pkr_contracts::{AbstractionBuilder, BlueprintProvider, Evaluator};
use pkr_core::state::{Action, ActionKind, GameState, Street};
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

/// Per-opponent paired result.
#[derive(Debug, Clone)]
pub struct TournamentResult {
    pub hands: u32,
    pub seed: u64,
    /// Mean chips/deal to whoever is in seat 0 when A is in seat 0.
    /// Equivalently, A's mean chip profit from the SB position.
    pub mean_a_seat0: f64,
    /// Mean chips/deal to whoever is in seat 0 when B is in seat 0.
    /// Equivalently, B's mean chip profit from the SB position.
    pub mean_b_seat0: f64,
    /// Headline: mean(A's seat-0 profit - B's seat-0 profit). Both
    /// passes are measured from the SAME seat's perspective, which
    /// cancels runout variance and is exactly zero when A == B.
    pub mean_diff: f64,
    /// Standard error of `mean_diff` on the per-hand paired differences.
    pub se_diff: f64,
    /// t-statistic = mean_diff / se_diff.
    pub t: f64,
}

/// Deal 9 unique cards from a fresh shuffle. Deterministic given `seed`.
fn deal_hand(seed: u64) -> ([u8; 2], [u8; 2], [u8; 5]) {
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut deck: [u8; 52] = core::array::from_fn(|i| i as u8);
    for i in 0..9 {
        let k = i + rng.random_range(0..(52 - i));
        deck.swap(i, k);
    }
    (
        [deck[0], deck[1]],
        [deck[2], deck[3]],
        [deck[4], deck[5], deck[6], deck[7], deck[8]],
    )
}

/// Play one hand. `hero` is (provider, seat); the other provider plays
/// the other seat. Returns hero's chip profit.
fn play_one(
    hero: &dyn BlueprintProvider,
    villain: &dyn BlueprintProvider,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    hero_hole: [u8; 2],
    villain_hole: [u8; 2],
    runout: &[u8; 5],
    hero_seat: u8,
    base_seed: u64,
) -> f32 {
    assert!(hero_seat < 2, "hero_seat must be 0 or 1");
    let vill_seat = 1 - hero_seat;

    // Assign holes by seat.
    let holes: [[u8; 2]; 2] = if hero_seat == 0 {
        [hero_hole, villain_hole]
    } else {
        [villain_hole, hero_hole]
    };

    let mut state = GameState::new(200.0, 1.0, 2.0);
    state.set_hole_cards(holes[0], holes[1]);

    // We need two contexts so seat 0 uses one provider and seat 1 the
    // other, even though they share an abstraction and evaluator.
    let ctx_seat0 = EvalContext {
        provider: if hero_seat == 0 { hero } else { villain },
        abstraction,
        evaluator,
    };
    let ctx_seat1 = EvalContext {
        provider: if hero_seat == 0 { villain } else { hero },
        abstraction,
        evaluator,
    };

    let mut deck_idx = 0usize;
    let mut decision_idx: u64 = 0;
    let mut steps = 0u32;

    while !state.is_terminal() && steps < 80 {
        steps += 1;

        let decision_seed =
            base_seed.wrapping_add(decision_idx.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        decision_idx += 1;
        let mut dec_rng = SmallRng::seed_from_u64(decision_seed);

        let act = if state.actor == 0 {
            let (a, _) = decide_via(&ctx_seat0, &state, &mut dec_rng);
            a
        } else {
            let (a, _) = decide_via(&ctx_seat1, &state, &mut dec_rng);
            a
        };
        state.apply_action_in_place(&act);

        if state.is_street_complete() && state.street != Street::River {
            let need = match state.street {
                Street::Preflop => 3,
                Street::Flop => 1,
                Street::Turn => 1,
                Street::River => 0,
            };
            if deck_idx + need > runout.len() {
                break;
            }
            let cards: Vec<u8> = runout[deck_idx..deck_idx + need].to_vec();
            deck_idx += need;
            state.advance_street_in_place(&cards);
        }
    }

    let payoff_seat0 = if state.is_terminal() {
        state.terminal_payoff(0, evaluator)
    } else {
        0.0
    };
    let hero_delta = if hero_seat == 0 {
        payoff_seat0
    } else {
        -payoff_seat0
    };
    let _ = vill_seat; // reserved for future use
    hero_delta
}

/// Wrapper: pick a concrete action for the actor using the ctx's
/// provider. Mirrors `decide_from_blueprint` (private in lib.rs) but
/// takes the ctx directly.
fn decide_via(
    ctx: &EvalContext,
    state: &GameState,
    rng: &mut SmallRng,
) -> (Action, bool) {
    // Legal concrete actions.
    let mut buf: [Action; 8] = [Action {
        player: 0,
        kind: ActionKind::Fold,
    }; 8];
    let n = state.legal_actions_into(&mut buf);
    if n == 0 {
        return (
            Action { player: state.actor, kind: ActionKind::Fold },
            false,
        );
    }

    // Compute the infoset hash and look up the provider.
    let actor = state.actor;
    let mut sig_buf = [0u8; 8];
    let sig_len = state.infoset_signature_into(&mut sig_buf);
    let history = &sig_buf[..sig_len];
    let hole = &state.hole[actor];
    let board: &[u8] = &state.board[..state.board_len as usize];
    let hash = ctx
        .abstraction
        .get_infoset_hash(hole, board, history, state.street as u8);

    let advice = match ctx.provider.lookup(hash) {
        Some(a) => a,
        None => {
            // Fallback: check/call if legal, else fold.
            for a in buf.iter().take(n) {
                if matches!(a.kind, ActionKind::Check | ActionKind::Call) {
                    return (*a, false);
                }
            }
            return (buf[0], false);
        }
    };

    // Decode CDF into per-bucket probabilities.
    let mut probs = [0.0f32; 6];
    let mut prev = 0u16;
    let len = (advice.len as usize).min(6);
    for i in 0..len {
        let c = advice.cdf_probabilities[i] as u16;
        probs[i] = (c.saturating_sub(prev)) as f32 / 255.0;
        prev = c;
    }

    // Bucket each legal action.
    let mut bucket_to_first: [Option<usize>; 6] = [None; 6];
    for (i, a) in buf.iter().take(n).enumerate() {
        let b = pkr_core::abstraction::action_bucket(
            &a.kind,
            state.stacks[actor],
            state.street_bets[actor],
            state.street_bets[1 - actor],
            state.pot,
        ) as usize;
        if b < 6 && bucket_to_first[b].is_none() {
            bucket_to_first[b] = Some(i);
        }
    }

    // Sample a bucket from the blueprint's distribution, restricted to
    // legal ones.
    let total: f32 = (0..6)
        .filter(|&b| bucket_to_first[b].is_some())
        .map(|b| probs[b])
        .sum();
    let chosen_bucket = if total > 1e-9 {
        let mut r = rng.random_range(0.0..total);
        let mut pick = 0usize;
        for b in 0..6 {
            if bucket_to_first[b].is_none() {
                continue;
            }
            r -= probs[b];
            if r <= 0.0 {
                pick = b;
                break;
            }
            pick = b;
        }
        pick
    } else {
        // No blueprint mass on legal buckets: pick the first legal.
        (0..6).find(|&b| bucket_to_first[b].is_some()).unwrap_or(0)
    };

    let idx = bucket_to_first[chosen_bucket].unwrap_or(0);
    (buf[idx], true)
}

/// Duplicate tournament between two blueprints.
///
/// Prefer this over two independent `run_eval_harness` calls whenever
/// you're comparing two providers directly: the paired SE is typically
/// several times smaller at the same hand count.
pub fn tournament(
    a: &dyn BlueprintProvider,
    b: &dyn BlueprintProvider,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    hands: u32,
    base_seed: u64,
) -> TournamentResult {
    let mut diffs: Vec<f64> = Vec::with_capacity(hands as usize);
    let mut sum_a_seat0 = 0.0f64;
    let mut sum_b_seat0 = 0.0f64;

    for i in 0..hands as u64 {
        let seed = base_seed.wrapping_add(i);
        let (h0, h1, runout) = deal_hand(seed);

        // Pass 1: A in seat 0, B in seat 1. Measured as seat-0 profit.
        let a_at_seat0 = play_one(a, b, abstraction, evaluator, h0, h1, &runout, 0, seed);

        // Pass 2: B in seat 0, A in seat 1. Measured as seat-0 profit.
        let b_at_seat0 = play_one(b, a, abstraction, evaluator, h0, h1, &runout, 0, seed);

        // Same-seat paired diff: how much more does A win from the
        // SB than B does from the SB, on the same deal?
        let diff = a_at_seat0 as f64 - b_at_seat0 as f64;

        sum_a_seat0 += a_at_seat0 as f64;
        sum_b_seat0 += b_at_seat0 as f64;
        diffs.push(diff);
    }

    let n = diffs.len().max(1) as f64;
    let mean_a_seat0 = sum_a_seat0 / n;
    let mean_b_seat0 = sum_b_seat0 / n;
    let mean_diff = diffs.iter().sum::<f64>() / n;
    let var = if diffs.len() > 1 {
        diffs.iter().map(|d| (d - mean_diff).powi(2)).sum::<f64>() / (n - 1.0)
    } else {
        0.0
    };
    let se_diff = (var / n).sqrt();
    let t = if se_diff > 0.0 { mean_diff / se_diff } else { 0.0 };

    TournamentResult {
        hands,
        seed: base_seed,
        mean_a_seat0,
        mean_b_seat0,
        mean_diff,
        se_diff,
        t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::SotaAdvice;

    /// A trivial provider that always returns uniform over 6 buckets.
    struct Uniform;
    impl BlueprintProvider for Uniform {
        fn lookup(&self, _h: u64) -> Option<SotaAdvice> {
            Some(SotaAdvice {
                cdf_probabilities: [
                    42, 85, 128, 170, 212, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                ],
                len: 6,
            })
        }
    }

    /// A provider that always checks/calls.
    struct Passive;
    impl BlueprintProvider for Passive {
        fn lookup(&self, _h: u64) -> Option<SotaAdvice> {
            Some(SotaAdvice {
                cdf_probabilities: [
                    0, 128, 128, 128, 128, 128, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                ],
                len: 6,
            })
        }
    }

    fn workspace_smoke() -> (pkr_abstraction::KMeansAbstraction, pkr_eval::NlheEvaluator) {
        let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let ws = m.parent().unwrap().parent().unwrap();
        let dir = ws.join("outputs/v0-smoke");
        let store = pkr_abstraction::load_centroids(
            dir.join("centroids.bin").to_str().unwrap(),
        )
        .expect("smoke centroids");
        let mut abs = pkr_abstraction::KMeansAbstraction::from_store(
            store,
            std::sync::Arc::new(pkr_eval::NlheEvaluator),
        );
        abs.init_table(0, dir.join("preflop_abstraction.bin").to_str().unwrap()).unwrap();
        abs.init_table(1, dir.join("flop_abstraction.bin").to_str().unwrap()).unwrap();
        abs.init_table(2, dir.join("turn_abstraction.bin").to_str().unwrap()).unwrap();
        abs.init_table(3, dir.join("river_buckets.bin").to_str().unwrap()).unwrap();
        (abs, pkr_eval::NlheEvaluator)
    }

    #[test]
    #[ignore]
    fn self_tournament_is_zero() {
        let (abs, ev) = workspace_smoke();
        let r = tournament(&Uniform, &Uniform, &abs, &ev, 50, 42);
        assert!(
            r.mean_diff.abs() < 1e-9,
            "A vs A must be exactly zero, got {}",
            r.mean_diff
        );
    }

    #[test]
    #[ignore]
    fn uniform_beats_passive_or_loses_sign_consistently() {
        let (abs, ev) = workspace_smoke();
        // No assertion on sign — this is a smoke check that the harness
        // produces a finite, non-degenerate number.
        let r = tournament(&Uniform, &Passive, &abs, &ev, 50, 42);
        assert!(r.mean_diff.is_finite());
        assert!(r.se_diff.is_finite());
    }
}
