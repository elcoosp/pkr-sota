//! Live action translation for the runtime (§S7).
//!
//! The trainer's abstract game offers pot-fraction bet anchors
//! `{0.5 → bucket 2, 1.0 → bucket 3, 2.0 → bucket 4}` plus jam
//! (`→ bucket 5`). A live opponent bet of size `x` (in the same
//! raise-above-call / pre-call-pot fraction units as
//! [`pkr_core::abstraction::action_bucket`]) usually falls *between*
//! anchors. Hard-thresholding it (the old runtime behaviour) is a known
//! exploit vector: an opponent can sit just below a threshold and always
//! be misclassified.
//!
//! Instead, translate probabilistically per Ganzfried & Sandholm (2013):
//! with neighbouring anchors `(a, b)` the bet maps to the lower anchor
//! with the pseudo-harmonic probability
//!
//! ```text
//! f(x) = (b − x)(1 + a) / ((b − a)(1 + x))
//! ```
//!
//! and to the upper anchor otherwise. At the boundaries the mapping is
//! deterministic (`x ≤ a → lower`, `x ≥ b → upper`).
//!
//! This replaces the dead [`pkr_export::translate`] reach-weighted linear
//! blend (which was never wired to the runtime and is now deprecated).

use rand::RngExt;

/// Pseudo-harmonic lower-anchor probability (Ganzfried & Sandholm 2013).
///
/// `f(x) = (b−x)(1+a) / ((b−a)(1+x))`, clamped to `[0, 1]`.
///
/// - `x <= a` → `1.0` (deterministically the lower anchor).
/// - `x >= b` → `0.0` (deterministically the upper anchor).
/// - Degenerate window (`b <= a`) → `0.5`.
pub fn pseudo_harmonic_prob_lower(a: f32, b: f32, x: f32) -> f32 {
    if b <= a {
        return 0.5;
    }
    if x <= a {
        return 1.0;
    }
    if x >= b {
        return 0.0;
    }
    if !b.is_finite() {
        // Unbounded upper window (jam disabled): the bet is always
        // closer to the lower anchor in pseudo-harmonic terms.
        return 1.0;
    }
    let p = (b - x) * (1.0 + a) / ((b - a) * (1.0 + x));
    p.clamp(0.0, 1.0)
}

/// Translate a live bet fraction to an abstract bucket (2..=5).
///
/// `x` is the raise-above-call size divided by the pre-call pot (same
/// units as [`pkr_core::abstraction::action_bucket`]). `jam_frac` is the
/// fraction at or above which the bet is an effective jam — i.e.
/// `(stacks + street_bets − max(street_bets)) / pot` for the actor;
/// callers that don't know the stacks pass `f32::INFINITY` to disable
/// the jam window (bets above 2.0 then map to bucket 4).
///
/// Behaviour:
/// - `x <= 0.5` → `2`; `x >= jam_frac` → `5` (both deterministic).
/// - Otherwise the bet falls in one of the windows
///   `[0.5, 1.0]`, `[1.0, 2.0]`, `[2.0, jam_frac]` and is randomized to
///   the window's lower/upper bucket with
///   [`pseudo_harmonic_prob_lower`]. Window endpoints map
///   deterministically to their own bucket.
pub fn translate_bet(x: f32, jam_frac: f32, rng: &mut impl rand::Rng) -> u8 {
    // Anchor fractions and their buckets. The jam anchor moves with the
    // stacks; the pot-fraction anchors are the abstract game's sizings.
    let jam = if jam_frac.is_finite() { jam_frac } else { f32::INFINITY };
    if x <= 0.5 {
        return 2;
    }
    if x >= jam {
        return 5;
    }
    let (a, b, lower, upper) = if x < 1.0 {
        (0.5f32, 1.0f32, 2u8, 3u8)
    } else if x < 2.0 {
        (1.0f32, 2.0f32, 3u8, 4u8)
    } else {
        (2.0f32, jam, 4u8, 5u8)
    };
    let p_lower = pseudo_harmonic_prob_lower(a, b, x);
    if rng.random::<f32>() < p_lower {
        lower
    } else {
        upper
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn boundaries_are_deterministic() {
        // x <= a → 1, x >= b → 0, for several windows.
        for (a, b) in [(0.5f32, 1.0f32), (1.0, 2.0), (2.0, 10.0)] {
            assert_eq!(pseudo_harmonic_prob_lower(a, b, a), 1.0);
            assert_eq!(pseudo_harmonic_prob_lower(a, b, a - 0.25), 1.0);
            assert_eq!(pseudo_harmonic_prob_lower(a, b, b), 0.0);
            assert_eq!(pseudo_harmonic_prob_lower(a, b, b + 1.0), 0.0);
        }
    }

    #[test]
    fn midpoint_sanity() {
        // f must be strictly inside (0,1) strictly between the anchors,
        // and decreasing in x.
        let p_lo = pseudo_harmonic_prob_lower(0.5, 1.0, 0.6);
        let p_mid = pseudo_harmonic_prob_lower(0.5, 1.0, 0.75);
        let p_hi = pseudo_harmonic_prob_lower(0.5, 1.0, 0.9);
        assert!(p_lo > p_mid && p_mid > p_hi, "{p_lo} {p_mid} {p_hi}");
        assert!(p_mid > 0.0 && p_mid < 1.0);
        // Pseudo-harmonic is top-heavy vs linear here: at x=0.75 the
        // linear weight on the lower anchor is 0.5; f should differ.
        let linear = (1.0 - 0.75) / (1.0 - 0.5);
        assert!((p_mid - linear).abs() > 1e-3, "p_mid={p_mid}");
    }

    #[test]
    fn degenerate_window_returns_half() {
        assert_eq!(pseudo_harmonic_prob_lower(1.0, 1.0, 1.5), 0.5);
    }

    #[test]
    fn translate_bet_deterministic_at_boundaries() {
        let mut rng = SmallRng::seed_from_u64(42);
        // Below/at the small anchor → 2 regardless of draw.
        for _ in 0..25 {
            assert_eq!(translate_bet(0.5, 8.0, &mut rng), 2);
            assert_eq!(translate_bet(0.1, 8.0, &mut rng), 2);
        }
        // At pot-fraction anchors → the anchor's own bucket.
        for _ in 0..25 {
            assert_eq!(translate_bet(1.0, 8.0, &mut rng), 3);
            assert_eq!(translate_bet(2.0, 8.0, &mut rng), 4);
        }
        // At/above jam → 5.
        for _ in 0..25 {
            assert_eq!(translate_bet(8.0, 8.0, &mut rng), 5);
            assert_eq!(translate_bet(20.0, 8.0, &mut rng), 5);
        }
    }

    #[test]
    fn translate_bet_stays_within_window() {
        // Interior points must land on one of the window's two buckets.
        let mut rng = SmallRng::seed_from_u64(7);
        for _ in 0..200 {
            let b = translate_bet(0.75, 8.0, &mut rng);
            assert!(b == 2 || b == 3, "b={b}");
            let b = translate_bet(1.5, 8.0, &mut rng);
            assert!(b == 3 || b == 4, "b={b}");
            let b = translate_bet(4.0, 8.0, &mut rng);
            assert!(b == 4 || b == 5, "b={b}");
        }
    }

    #[test]
    fn translate_bet_randomizes_interior() {
        // An interior point must produce both buckets over many draws
        // (randomization is the point of §S7).
        let mut rng = SmallRng::seed_from_u64(1234);
        let mut saw_lo = false;
        let mut saw_hi = false;
        for _ in 0..200 {
            match translate_bet(0.75, 8.0, &mut rng) {
                2 => saw_lo = true,
                3 => saw_hi = true,
                b => panic!("out of window: {b}"),
            }
        }
        assert!(saw_lo && saw_hi, "no randomization observed");
    }

    #[test]
    fn infinite_jam_frac_disables_jam_window() {
        let mut rng = SmallRng::seed_from_u64(99);
        for _ in 0..50 {
            assert_eq!(translate_bet(5.0, f32::INFINITY, &mut rng), 4);
        }
    }
}
