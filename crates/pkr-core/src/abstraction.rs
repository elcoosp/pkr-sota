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
use bytemuck::{Pod, Zeroable};

/// Number of abstract action buckets.
pub const NUM_ACTION_BUCKETS: usize = 6;

/// Concrete bet sizings in pot fractions. Length must match the
/// `Bet` actions produced by `legal_actions*`.
pub const BET_SIZINGS: [f32; 3] = [0.5, 1.0, 2.0];

/// River hand-tier quantization shift (T2.2, audit F6 follow-up).
///
/// The river infoset hash quantizes the evaluator's raw rank by
/// `hand_rank >> RIVER_TIER_SHIFT`. Changing this value changes every
/// river infoset key, so the fingerprint must reflect it or a checkpoint
/// could silently load into a binary that computes different hashes.
///
/// History: 6 -> 15 (audit F6) -> 13 (T2.2, reverted 2026-09-24 -- T2.2
/// increased infoset count ~2.5x without measurable quality improvement).
pub const RIVER_TIER_SHIFT: u8 = 15;

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

// ---------------------------------------------------------------------------
// F2a: abstraction fingerprint (r3 F2)
// ---------------------------------------------------------------------------
//
// Semantic changes that alter infoset identity or action meaning:
//   - bucket sizing constants (BET_SIZINGS)
//   - bucket thresholds (BUCKET_THRESHOLD_*)
//   - signature version (SIG_V2_STREET_MONEY / SIG_V2_INCLUDE_LBF)
//   - hash algorithm
//   - the preflop cluster count `k` (and, in future, flop/river k)
//
// Resuming a checkpoint across any of these silently corrupts training:
// the regrets were accumulated against a *different* game. This struct
// is written into every checkpoint and blueprint, and compared on load.
// Mismatch ⇒ hard error, telling the operator to delete the checkpoint.
//
// It is deliberately a POD `#[repr(C)]` struct with fixed layout so it
// can be written byte-for-byte into both on-disk formats without
// per-format serialization logic.

/// Semantic-configuration fingerprint. 40 bytes, `#[repr(C)]`, POD.
///
/// Compare with `==`. Field order is part of the on-disk format; do not
/// reorder without a format-version bump.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Pod, Zeroable)]
pub struct AbstractionFingerprint {
    /// Number of preflop centroids / buckets (`centroids.bin` size).
    pub preflop_k: u32,
    /// Number of flop board buckets (currently untracked; 0).
    pub flop_k: u32,
    /// Number of river board buckets (currently untracked; 0).
    pub river_buckets: u32,
    /// Pot-fraction sizings, must equal `BET_SIZINGS`.
    pub sizing_small: f32,
    pub sizing_medium: f32,
    pub sizing_large: f32,
    /// Bucket thresholds, must equal `BUCKET_THRESHOLD_*`.
    pub threshold_small: f32,
    pub threshold_large: f32,
    /// Signature version: 1 = legacy 24-bit, 2 = SPR+faced-bet-size v2.
    pub sig_version: u8,
    /// Hash algorithm identifier (`HASH_ALGO_FNV1A64_INFOSET`).
    pub hash_algo: u8,
    /// T2.2: river hand-tier shift (`hand_rank >> RIVER_TIER_SHIFT`).
    /// Older writers left `_pad[0] = 0`, which is a distinct value from
    /// any valid shift, so a pre-T2.2 checkpoint loaded against a post-
    /// T2.2 binary produces a fingerprint mismatch (correct behaviour).
    pub river_tier_shift: u8,
    /// F6: action-legality version. 0 = pre-F6 (raise cap forbade the
    /// jam, pot-fraction sizes unclamped). 1 = F6 (jam always legal,
    /// raises clamped to the true min-raise-to). Old checkpoints have
    /// `_pad[0] = 0` here, so a pre-F6 checkpoint loaded against a
    /// post-F6 binary produces a fingerprint mismatch — as it must,
    /// because the abstract game changed.
    pub action_legal_v: u8,
    /// F4: centroid feature-space version. 0 = legacy (EHS, EHS²)
    /// centroids, the current tables. 1 = (mean, potential) from
    /// `pkr_abstraction::potential::ehs_and_potential`. Every existing
    /// checkpoint is 0; the F4 rebuild will write 1.
    ///
    /// This exists so a checkpoint trained on one feature space
    /// refuses to load against tables built for the other. Before the
    /// audit there was no such guard: the fingerprint recorded `k` but
    /// not what the centroids meant, so a hand-fit on one feature set
    /// could be loaded against tables from another and silently
    /// produce nonsense infosets.
    pub centroid_feature_v: u8,
    pub _pad: [u8; 3],
}

impl AbstractionFingerprint {
    /// Build a fingerprint from the current compile-time constants plus
    /// the runtime-discovered preflop cluster count.
    ///
    /// `preflop_k` comes from `centroids.bin` at trainer startup; pass
    /// the same value to every writer so a resumed checkpoint can
    /// detect a k change.
    ///
    /// `flop_k` and `river_buckets` are currently `0` (untracked).
    /// Wiring them requires the trainer to know the abstraction table
    /// dimensions; the F2b commit keeps this constructor's signature
    /// stable and adds those fields when needed.
    pub fn from_constants(preflop_k: u32) -> Self {
        // F4: which feature space the centroid tables describe.
        //
        //   0 = legacy (EHS, EHS²), every pre-F4 checkpoint
        //   1 = (mean, potential) from pkr_abstraction::potential
        //
        // Read from PKR_CENTROID_FEATURE_V so the trainer, arena, and
        // tournament all pick the same value without any plumbing. The
        // launcher for an F4 run exports it once. Defaults to 0 so
        // every existing path is unchanged.
        let centroid_feature_v: u8 = std::env::var("PKR_CENTROID_FEATURE_V")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        Self {
            preflop_k,
            flop_k: 0,
            river_buckets: 0,
            sizing_small: BET_SIZINGS[0],
            sizing_medium: BET_SIZINGS[1],
            sizing_large: BET_SIZINGS[2],
            threshold_small: BUCKET_THRESHOLD_SMALL,
            threshold_large: BUCKET_THRESHOLD_LARGE,
            sig_version: if crate::state::SIG_V3_SIZE_AWARE {
                3
            } else if crate::state::SIG_V2_STREET_MONEY {
                2
            } else {
                1
            },
            hash_algo: pkr_contracts::HASH_ALGO_FNV1A64_INFOSET,
            river_tier_shift: RIVER_TIER_SHIFT,
            action_legal_v: 1,
            centroid_feature_v,
            _pad: [0; 3],
        }
    }

    /// Human-readable mismatch report. Called on checkpoint load when
    /// the stored fingerprint differs from the current one.
    pub fn describe_mismatch(&self, expected: &Self) -> String {
        if self.river_tier_shift != expected.river_tier_shift {
            return format!(
                "river_tier_shift mismatch: stored={} current={} (T2.2 changed the \
                 river hash shift; retrain or use --fresh)",
                self.river_tier_shift, expected.river_tier_shift,
            );
        }
        if self.action_legal_v != expected.action_legal_v {
            return format!(
                "action_legal_v mismatch: stored={} current={} (F6 changed the \
                 legal action tree — the raise cap no longer forbids the jam and \
                 pot-fraction raises are clamped to the min-raise-to; retrain or \
                 use --fresh)",
                self.action_legal_v, expected.action_legal_v,
            );
        }
        if self.centroid_feature_v != expected.centroid_feature_v {
            return format!(
                "centroid_feature_v mismatch: stored={} current={} (the centroid \
                 feature space changed — 0 = legacy (EHS, EHS²), 1 = (mean, \
                 potential). Retrain or use --fresh against the matching tables.)",
                self.centroid_feature_v, expected.centroid_feature_v,
            );
        }
        let mut diffs = Vec::new();
        if self.preflop_k != expected.preflop_k {
            diffs.push(format!(
                "preflop_k: {} (checkpoint) vs {} (current)",
                self.preflop_k, expected.preflop_k
            ));
        }
        if self.flop_k != expected.flop_k {
            diffs.push(format!("flop_k: {} vs {}", self.flop_k, expected.flop_k));
        }
        if self.river_buckets != expected.river_buckets {
            diffs.push(format!(
                "river_buckets: {} vs {}",
                self.river_buckets, expected.river_buckets
            ));
        }
        if self.sizing_small != expected.sizing_small
            || self.sizing_medium != expected.sizing_medium
            || self.sizing_large != expected.sizing_large
        {
            diffs.push(format!(
                "sizings: [{}, {}, {}] vs [{}, {}, {}]",
                self.sizing_small,
                self.sizing_medium,
                self.sizing_large,
                expected.sizing_small,
                expected.sizing_medium,
                expected.sizing_large,
            ));
        }
        if self.threshold_small != expected.threshold_small
            || self.threshold_large != expected.threshold_large
        {
            diffs.push(format!(
                "thresholds: [{}, {}] vs [{}, {}]",
                self.threshold_small,
                self.threshold_large,
                expected.threshold_small,
                expected.threshold_large,
            ));
        }
        if self.sig_version != expected.sig_version {
            diffs.push(format!(
                "sig_version: {} vs {}",
                self.sig_version, expected.sig_version
            ));
        }
        if self.hash_algo != expected.hash_algo {
            diffs.push(format!(
                "hash_algo: {} vs {}",
                self.hash_algo, expected.hash_algo
            ));
        }
        if diffs.is_empty() {
            "no differences (this should not have been called)".to_string()
        } else {
            diffs.join("; ")
        }
    }
}

/// Compile-time size guard: the fingerprint must be exactly 40 bytes so
/// the checkpoint reader can `read_exact` it without knowing the layout.
const _: () = assert!(std::mem::size_of::<AbstractionFingerprint>() == 40);

#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    #[test]
    fn from_constants_captures_current_compile_time_values() {
        let f = AbstractionFingerprint::from_constants(200);
        assert_eq!(f.preflop_k, 200);
        assert_eq!(f.flop_k, 0);
        assert_eq!(f.river_buckets, 0);
        assert_eq!(f.sizing_small, BET_SIZINGS[0]);
        assert_eq!(f.sizing_medium, BET_SIZINGS[1]);
        assert_eq!(f.sizing_large, BET_SIZINGS[2]);
        assert_eq!(f.threshold_small, BUCKET_THRESHOLD_SMALL);
        assert_eq!(f.threshold_large, BUCKET_THRESHOLD_LARGE);
        let expected_sig = if crate::state::SIG_V2_STREET_MONEY {
            2
        } else {
            1
        };
        assert_eq!(f.sig_version, expected_sig);
        assert_eq!(f.hash_algo, pkr_contracts::HASH_ALGO_FNV1A64_INFOSET);
    }

    #[test]
    fn identical_inputs_produce_equal_fingerprints() {
        let a = AbstractionFingerprint::from_constants(200);
        let b = AbstractionFingerprint::from_constants(200);
        assert_eq!(a, b);
    }

    #[test]
    fn k_change_produces_inequality() {
        let a = AbstractionFingerprint::from_constants(200);
        let b = AbstractionFingerprint::from_constants(8);
        assert_ne!(a, b);
    }

    #[test]
    fn describe_mismatch_names_the_difference() {
        let a = AbstractionFingerprint::from_constants(200);
        let b = AbstractionFingerprint::from_constants(8);
        let msg = a.describe_mismatch(&b);
        assert!(msg.contains("preflop_k"), "msg: {msg}");
        assert!(msg.contains("200"), "msg: {msg}");
        assert!(msg.contains("8"), "msg: {msg}");
    }

    #[test]
    fn describe_mismatch_sizings() {
        let mut a = AbstractionFingerprint::from_constants(200);
        let b = AbstractionFingerprint::from_constants(200);
        a.sizing_small = 0.33;
        let msg = a.describe_mismatch(&b);
        assert!(msg.contains("sizings"), "msg: {msg}");
    }

    #[test]
    fn describe_mismatch_sig_version() {
        // Force a mismatch regardless of the current default value of
        // SIG_V2_STREET_MONEY. Pick a value different from the one
        // `from_constants` produces for `b`, without assuming which
        // value that is.
        let mut a = AbstractionFingerprint::from_constants(200);
        let b = AbstractionFingerprint::from_constants(200);
        a.sig_version = if b.sig_version == 1 { 2 } else { 1 };
        let msg = a.describe_mismatch(&b);
        assert!(msg.contains("sig_version"), "msg: {msg}");
    }

    #[test]
    fn pod_roundtrip_preserves_all_fields() {
        let f = AbstractionFingerprint::from_constants(200);
        let bytes = bytemuck::bytes_of(&f);
        assert_eq!(bytes.len(), 40);
        let back: &AbstractionFingerprint = bytemuck::from_bytes(bytes);
        assert_eq!(*back, f);
    }
}

#[cfg(test)]
mod fingerprint_sensitivity_tests {
    use super::*;

    /// Every semantic axis must produce a *different* fingerprint. If
    /// any of these assertions ever fails, the fingerprint is not
    /// covering what it claims to cover, and stale checkpoints could
    /// silently resume under a changed game.
    #[test]
    fn fingerprint_distinguishes_each_semantic_axis() {
        let base = AbstractionFingerprint::from_constants(200);
        let cases: &[(&str, AbstractionFingerprint)] = &[
            (
                "preflop_k",
                AbstractionFingerprint {
                    preflop_k: 201,
                    ..base
                },
            ),
            ("flop_k", AbstractionFingerprint { flop_k: 1, ..base }),
            (
                "river_buckets",
                AbstractionFingerprint {
                    river_buckets: 1,
                    ..base
                },
            ),
            (
                "sizing_small",
                AbstractionFingerprint {
                    sizing_small: base.sizing_small + 0.1,
                    ..base
                },
            ),
            (
                "sizing_medium",
                AbstractionFingerprint {
                    sizing_medium: base.sizing_medium + 0.1,
                    ..base
                },
            ),
            (
                "sizing_large",
                AbstractionFingerprint {
                    sizing_large: base.sizing_large + 0.1,
                    ..base
                },
            ),
            (
                "threshold_small",
                AbstractionFingerprint {
                    threshold_small: base.threshold_small + 0.1,
                    ..base
                },
            ),
            (
                "threshold_large",
                AbstractionFingerprint {
                    threshold_large: base.threshold_large + 0.1,
                    ..base
                },
            ),
            (
                "sig_version",
                AbstractionFingerprint {
                    sig_version: base.sig_version.wrapping_add(1),
                    ..base
                },
            ),
            (
                "hash_algo",
                AbstractionFingerprint {
                    hash_algo: base.hash_algo.wrapping_add(1),
                    ..base
                },
            ),
        ];
        for (name, modified) in cases {
            assert_ne!(
                base, *modified,
                "fingerprint did not distinguish change to `{name}`"
            );
            let msg = base.describe_mismatch(modified);
            assert!(
                msg.contains(name) || name.starts_with("sizing") || name.starts_with("threshold"),
                "describe_mismatch must name `{name}`, got: {msg}"
            );
        }
    }

    /// Identical field values produce identical fingerprints.
    #[test]
    fn fingerprint_equality_is_structural() {
        let a = AbstractionFingerprint::from_constants(200);
        let mut b = AbstractionFingerprint::from_constants(200);
        assert_eq!(a, b);
        b.sizing_small += 1e-9;
        // f32: 1e-9 won't change the value at 0.5. Confirm.
        assert_eq!(a.sizing_small, b.sizing_small);
        assert_eq!(a, b);
        b.sizing_small += 0.5;
        assert_ne!(a, b);
    }

    /// Fingerprint encodes the size of the PREFLOF cluster count as
    /// `u32`. Verify k=8 vs k=200 are distinct (the §17 incident).
    #[test]
    fn fingerprint_catches_k8_leak() {
        let good = AbstractionFingerprint::from_constants(200);
        let bad = AbstractionFingerprint::from_constants(8);
        assert_ne!(good, bad);
        assert!(good.describe_mismatch(&bad).contains("preflop_k"));
    }

    /// Fingerprint version-1 construction (sig_version=1) is the default
    /// with SIG_V2_STREET_MONEY=false.
    #[test]
    fn fingerprint_reports_current_sig_version() {
        let fp = AbstractionFingerprint::from_constants(200);
        let expected = if crate::state::SIG_V2_STREET_MONEY {
            2
        } else {
            1
        };
        assert_eq!(fp.sig_version, expected);
    }
}
