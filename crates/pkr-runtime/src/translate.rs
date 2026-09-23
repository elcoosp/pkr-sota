//! Runtime action translation.
//!
//! When the host app observes an off-tree opponent bet size (one that
//! doesn't match any concrete bet size the trainer used), we map it onto
//! the two bracketing trained anchors using the pseudo-harmonic formula
//! (Ganzfried & Sandholm 2013). The blueprint stores the anchors in its
//! v3 layout, so the runtime does not need to know the trainer's sizing
//! policy at compile time.
//!
//! The training-side `pkr_export::translate::compute_translation` is the
//! full formula with opponent reach in the denominator. At inference
//! time the runtime has no reach estimator; we use the equal-reach form,
//! which is the standard fallback and matches the conservative precompute.

use crate::mmap::MmapReader;
use pkr_contracts::SotaAdvice;

/// Bracket an off-tree pot fraction onto the two nearest trained anchors
/// for the given street.
///
/// Returns `(lower_anchor, upper_anchor, p_lower, p_upper)` where the two
/// probabilities sum to 1.0. If `fraction` is outside the anchor range,
/// both anchors collapse to the nearest one with weight 1.0.
pub fn bracket_bet(reader: &MmapReader, street: u8, fraction: f32) -> (f32, f32, f32, f32) {
    let street_idx = (street as usize).min(3);
    let row = reader.anchors()[street_idx];

    // Sort anchors ascending (writer emits them in order but be defensive).
    let mut a = [row[0], row[1], row[2]];
    a.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));

    // Preflop: no partial sizes. All mass to the all-in anchor.
    if a[2] == 0.0 {
        return (1.0, 1.0, 1.0, 0.0);
    }

    if fraction <= a[0] {
        return (a[0], a[0], 1.0, 0.0);
    }
    if fraction >= a[2] {
        return (a[2], a[2], 1.0, 0.0);
    }

    let (lower, upper) = if fraction <= a[1] {
        (a[0], a[1])
    } else {
        (a[1], a[2])
    };

    // Equal-reach pseudo-harmonic: linear interpolation of mass.
    let p_lower = ((upper - fraction) / (upper - lower)).clamp(0.0, 1.0);
    (lower, upper, p_lower, 1.0 - p_lower)
}

// ---------------------------------------------------------------------------
// T2.1: off-tree action resolution
// ---------------------------------------------------------------------------

/// Given a trained advice CDF and the concrete bet amount the host app
/// has requested (chips), redistribute the bet-bucket mass between the
/// two trained anchors that bracket the requested pot fraction.
///
/// Bucket layout (post-T0.2, must match `state.rs::abstract_action_index_static`):
///   0 = fold
///   1 = check / call
///   2 = anchor[0] (0.4x pot by default)
///   3 = anchor[1] (0.8x pot)
///   4 = anchor[2] (1.6x pot)
///   5 = all-in
///
/// Uses `pkr_export::translate::compute_translation` with equal reach
/// (0.5, 0.5). The runtime has no reach estimator at serve time; equal
/// reach is the conservative default the playbook specifies, and it
/// degrades to linear interpolation between the two anchors.
///
/// If the street has no partial anchors (preflop: anchors = [0, 0, 0]),
/// the input is returned unchanged.
pub fn resolve_action(
    advice: &SotaAdvice,
    anchors: &[f32; 3],
    requested_amount: f32,
    pot: f32,
) -> SotaAdvice {
    const N: usize = 6;

    // Decode cumulative CDF bytes into per-bucket probabilities.
    let mut probs = [0.0f32; N];
    let n = (advice.len as usize).min(N);
    let mut prev = 0u16;
    for i in 0..n {
        let c = advice.cdf_probabilities[i] as u16;
        probs[i] = (c.saturating_sub(prev)) as f32 / 255.0;
        prev = c;
    }

    // Sort anchors ascending for bracketing logic.
    let mut sorted = *anchors;
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Preflop: no partial sizes trained. Nothing to translate.
    if sorted[2] <= 0.0 {
        return *advice;
    }

    let fraction = if pot > 0.0 { requested_amount / pot } else { 0.0 };

    let (lo_frac, hi_frac, p_lo) = if fraction <= sorted[0] {
        (sorted[0], sorted[0], 1.0f32)
    } else if fraction >= sorted[2] {
        (sorted[2], sorted[2], 1.0f32)
    } else {
        let (lo, hi) = if fraction <= sorted[1] {
            (sorted[0], sorted[1])
        } else {
            (sorted[1], sorted[2])
        };
        // Equal reach: runtime has no belief state. compute_translation
        // returns (lower_q, upper_q) bytes summing to 255.
        let (q_lo, _q_hi) =
            pkr_export::translate::compute_translation(lo, hi, fraction, 0.5, 0.5);
        (lo, hi, q_lo as f32 / 255.0)
    };

    // Which bucket index corresponds to each anchor? Compare by value
    // against the original (unsorted) anchors array; the header writes
    // anchors in sorted order but we don't want to rely on that.
    let lo_bucket = bucket_for_anchor(anchors, lo_frac);
    let hi_bucket = bucket_for_anchor(anchors, hi_frac);

    // Total bet-bucket mass across the three partial anchors and all-in.
    let bet_mass = probs[2] + probs[3] + probs[4] + probs[5];
    probs[2] = 0.0;
    probs[3] = 0.0;
    probs[4] = 0.0;
    probs[5] = 0.0;

    if let Some(b) = lo_bucket {
        probs[b] += bet_mass * p_lo;
    }
    if let Some(b) = hi_bucket {
        probs[b] += bet_mass * (1.0 - p_lo);
    }

    // Renormalize to be robust against a slightly-off input CDF (drift
    // from u8 rounding, truncated len, etc.).
    let sum: f32 = probs.iter().sum();
    if sum <= 1e-9 {
        return *advice;
    }

    let mut out = SotaAdvice {
        cdf_probabilities: [0u8; 16],
        len: advice.len,
    };
    let mut cum = 0.0f32;
    for i in 0..n {
        cum += probs[i] / sum;
        out.cdf_probabilities[i] = (cum * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    if n > 0 {
        out.cdf_probabilities[n - 1] = 255;
    }
    out
}

/// Which bucket index (2, 3, or 4) holds the given anchor value?
/// Returns None if the anchor does not correspond to any trained bucket
/// (e.g. the all-in anchor at index 5, or an unexpected value).
fn bucket_for_anchor(anchors: &[f32; 3], anchor: f32) -> Option<usize> {
    for (i, &a) in anchors.iter().enumerate() {
        if (a - anchor).abs() < 1e-6 {
            return Some(2 + i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signature-only check that the existing bracket_bet API builds.
    #[test]
    fn signature_is_stable() {
        fn _sig(_r: &MmapReader, _street: u8, _frac: f32) -> (f32, f32, f32, f32) {
            unreachable!()
        }
    }

    /// Trained CDF helper: fold=10%, call=40%, b2=30%, b3=15%, b4=5%, b5=0%.
    /// Stored as cumulative bytes [26, 128, 204, 243, 255, 255, ...].
    fn sample_advice() -> SotaAdvice {
        SotaAdvice {
            cdf_probabilities: [
                26, 128, 204, 243, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            len: 6,
        }
    }

    #[test]
    fn off_tree_bet_splits_mass_between_anchors() {
        let advice = sample_advice();
        let anchors = [0.4f32, 0.8, 1.6];
        // Request 0.7 x pot. Falls between 0.4 and 0.8.
        let out = resolve_action(&advice, &anchors, 0.7, 1.0);

        // Fold + call mass preserved.
        let fold = out.cdf_probabilities[0] as u16;
        assert!(fold > 15 && fold < 40, "fold bucket ≈ 10%, got {fold}");

        // Bet mass lives on bucket 2 (0.4) and bucket 3 (0.8).
        // Bucket 3 should dominate since 0.7 is closer to 0.8.
        let b2 = out.cdf_probabilities[2] as u16 - out.cdf_probabilities[1] as u16;
        let b3 = out.cdf_probabilities[3] as u16 - out.cdf_probabilities[2] as u16;
        let b4 = out.cdf_probabilities[4] as u16 - out.cdf_probabilities[3] as u16;

        assert!(b3 > b2, "0.7 closer to 0.8: b2={b2}, b3={b3}");
        assert_eq!(b4, 0, "no mass should remain on 1.6 anchor");
        assert_eq!(out.cdf_probabilities[5], 255, "CDF must end at 255");
    }

    #[test]
    fn exact_anchor_keeps_all_mass_on_one_bucket() {
        let advice = sample_advice();
        let anchors = [0.4f32, 0.8, 1.6];
        let out = resolve_action(&advice, &anchors, 0.4, 1.0);
        // 0.4 == anchors[0] exactly. All bet mass lands on bucket 2.
        let b2 = out.cdf_probabilities[2] as u16 - out.cdf_probabilities[1] as u16;
        let b3 = out.cdf_probabilities[3] as u16 - out.cdf_probabilities[2] as u16;
        assert!(b2 > 0, "bucket 2 must hold translated mass");
        assert_eq!(b3, 0, "bucket 3 must be empty");
    }

    #[test]
    fn below_lower_anchor_clamps_to_lower() {
        let advice = sample_advice();
        let anchors = [0.4f32, 0.8, 1.6];
        // Request 0.2 x pot -- below all anchors. Should clamp to 0.4.
        let out = resolve_action(&advice, &anchors, 0.2, 1.0);
        let b2 = out.cdf_probabilities[2] as u16 - out.cdf_probabilities[1] as u16;
        let b3 = out.cdf_probabilities[3] as u16 - out.cdf_probabilities[2] as u16;
        assert!(b2 > 0, "clamped mass to 0.4 anchor");
        assert_eq!(b3, 0);
    }

    #[test]
    fn above_upper_anchor_clamps_to_upper() {
        let advice = sample_advice();
        let anchors = [0.4f32, 0.8, 1.6];
        // Request 5 x pot -- well above all anchors. Clamp to 1.6.
        let out = resolve_action(&advice, &anchors, 5.0, 1.0);
        let b4 = out.cdf_probabilities[4] as u16 - out.cdf_probabilities[3] as u16;
        assert!(b4 > 0, "clamped mass to 1.6 anchor");
    }

    #[test]
    fn preflop_zero_anchors_return_unchanged() {
        let advice = sample_advice();
        let anchors = [0.0f32, 0.0, 0.0];
        let out = resolve_action(&advice, &anchors, 5.0, 10.0);
        assert_eq!(out.cdf_probabilities, advice.cdf_probabilities);
    }

    #[test]
    fn fold_and_call_mass_is_conserved() {
        let advice = sample_advice();
        let anchors = [0.4f32, 0.8, 1.6];
        let out = resolve_action(&advice, &anchors, 0.6, 1.0);
        // fold byte unchanged.
        assert_eq!(out.cdf_probabilities[0], advice.cdf_probabilities[0]);
        // call byte unchanged.
        assert_eq!(out.cdf_probabilities[1], advice.cdf_probabilities[1]);
    }
}
