//! Single source of truth for the 6-bucket action abstraction.
//!
//! Before this module there were three independent copies of the
//! bucket mapping with two different threshold sets. Any fix to one
//! could silently miss the others. Now: one function, one set of
//! constants.
//!
//! ## Bucket semantics
//!
//! A `Bet(amount)` action means "the actor's street-bet total becomes
//! `amount`". Bucketing measures the size of the wager relative to the
//! pot, where "size" is **the chips the actor adds beyond the current
//! call obligation** — NOT the total street commitment.
//!
//! For a bet (to_call = 0): size = amount - street_bets[actor].
//! For a raise (to_call > 0):  size = amount - opp_street_bets.
//!
//! Both reduce to `size = amount - max(street_bets[actor], opp_street_bets)`,
//! which is what this module uses. Dividing the *total commitment* by
//! pot instead (the pre-C3 formula) inflates preflop sizes by the
//! already-posted blind and collapses distinct sizings into one bucket.

use crate::state::ActionKind;

/// Number of abstract action buckets.
pub const NUM_ACTION_BUCKETS: usize = 6;

/// Concrete bet sizings in pot fractions. Length must match the
/// `Bet` actions produced by `legal_actions*`.
pub const BET_SIZINGS: [f32; 3] = [0.5, 1.0, 2.0];

/// Bucket 2: raise size fraction < BUCKET_THRESHOLD_SMALL.
pub const BUCKET_THRESHOLD_SMALL: f32 = 0.6;
/// Bucket 3: BUCKET_THRESHOLD_SMALL <= fraction < BUCKET_THRESHOLD_LARGE.
pub const BUCKET_THRESHOLD_LARGE: f32 = 1.2;

/// Map a concrete action to its abstract bucket (0..=5).
///
/// `stacks`, `street_bets`, `opp_street_bets`, and `pot` are the actor's
/// (and opponent's) state **before** the action is applied.
///
/// All 6 return values are legal for every input; callers that need to
/// know which buckets have a concrete representative should enumerate
/// `legal_actions*` and map each action through this function.
#[inline]
pub fn action_bucket(
    kind: &ActionKind,
    stacks: f32,
    street_bets: f32,
    opp_street_bets: f32,
    pot: f32,
) -> u8 {
    match kind {
        ActionKind::Fold => 0,
        ActionKind::Check | ActionKind::Call => 1,
        ActionKind::Bet(amount) => {
            if *amount >= stacks + street_bets {
                return 5;
            }
            // Chips the actor adds above the current call obligation.
            let committed_before = street_bets.max(opp_street_bets);
            let raise_size = (*amount - committed_before).max(0.0);
            let pot = if pot < 1.2 { 1.2 } else { pot };
            let fraction = raise_size / pot;
            if fraction < BUCKET_THRESHOLD_SMALL {
                2
            } else if fraction < BUCKET_THRESHOLD_LARGE {
                3
            } else {
                4
            }
        }
    }
}

#[cfg(test)]
mod c3_tests {
    use super::*;

    #[test]
    fn anchors_map_to_distinct_buckets_postflop() {
        // Postflop: no street bets, pot 4, stack 198.
        let (stacks, sb, opp) = (198.0f32, 0.0f32, 0.0f32);
        let pot = 4.0f32;
        for (frac, want) in [(0.5, 2u8), (1.0, 3), (2.0, 4)] {
            let amt = sb + pot * frac;
            let got = action_bucket(&ActionKind::Bet(amt), stacks, sb, opp, pot);
            assert_eq!(got, want, "postflop frac {frac} amt {amt}");
        }
        assert_eq!(
            action_bucket(&ActionKind::Bet(stacks + sb), stacks, sb, opp, pot),
            5,
            "all-in bucket"
        );
    }

    #[test]
    fn anchors_map_to_distinct_buckets_preflop_raise_over_limp() {
        // Preflop: BB (street_bets=2) raises over SB limp (opp=2),
        // pot = 4. The raise TOTAL is opp + pot*frac.
        let (stacks, sb, opp) = (198.0f32, 2.0f32, 2.0f32);
        let pot = 4.0f32;
        for (frac, want) in [(0.5, 2u8), (1.0, 3), (2.0, 4)] {
            let amt = opp + pot * frac;
            let got = action_bucket(&ActionKind::Bet(amt), stacks, sb, opp, pot);
            assert_eq!(got, want, "preflop limp frac {frac} amt {amt}");
        }
        assert_eq!(
            action_bucket(&ActionKind::Bet(stacks + sb), stacks, sb, opp, pot),
            5,
            "all-in bucket"
        );
    }

    #[test]
    fn sb_re_raise_over_bb_open() {
        // SB (street_bets=2) re-raises over BB's open to 6 (opp=6),
        // pot = 8. Total = opp + pot*frac.
        let (stacks, sb, opp) = (198.0f32, 2.0f32, 6.0f32);
        let pot = 8.0f32;
        for (frac, want) in [(0.5, 2u8), (1.0, 3), (2.0, 4)] {
            let amt = opp + pot * frac;
            let got = action_bucket(&ActionKind::Bet(amt), stacks, sb, opp, pot);
            assert_eq!(got, want, "re-raise frac {frac} amt {amt}");
        }
    }

    #[test]
    fn fold_and_call_buckets() {
        assert_eq!(action_bucket(&ActionKind::Fold, 100.0, 0.0, 0.0, 5.0), 0);
        assert_eq!(action_bucket(&ActionKind::Check, 100.0, 0.0, 0.0, 5.0), 1);
        assert_eq!(action_bucket(&ActionKind::Call, 100.0, 0.0, 0.0, 5.0), 1);
    }

    #[test]
    fn thresholds_are_injective_on_offered_sizings() {
        use std::collections::HashSet;
        // Both postflop and preflop raise configurations.
        let configs = [
            // (stacks, sb, opp, pot)
            (200.0f32, 0.0f32, 0.0f32, 4.0f32),
            (198.0, 2.0, 2.0, 4.0), // preflop limp
            (198.0, 2.0, 6.0, 8.0), // preflop re-raise
        ];
        for (stacks, sb, opp, pot) in configs {
            let mut seen = HashSet::new();
            for frac in BET_SIZINGS {
                let amt = opp.max(sb) + pot * frac;
                let b = action_bucket(&ActionKind::Bet(amt), stacks, sb, opp, pot);
                assert!(
                    seen.insert(b),
                    "cfg (sb={sb}, opp={opp}, pot={pot}) frac {frac} → bucket {b} collides"
                );
            }
            assert_eq!(seen.len(), 3);
        }
    }

    #[test]
    fn small_pot_clamps_to_1_2() {
        // pot=3.0 → clamped to 3.0 (>= 1.2 so no clamp). Bet 3.0,
        // no street bets: size = 3.0, frac = 1.0 → bucket 3.
        let b = action_bucket(&ActionKind::Bet(3.0), 199.0, 0.0, 0.0, 3.0);
        assert_eq!(b, 3);
        // Degenerate: pot 0 → clamped to 1.2; bet 0.6 → frac 0.5 → bucket 2.
        let b = action_bucket(&ActionKind::Bet(0.6), 199.0, 0.0, 0.0, 0.0);
        assert_eq!(b, 2);
    }
}
