/// Reach-weighted linear interpolation for off-tree action translation.
///
/// **⚠️ NOT the Ganzfried & Sandholm (2013) pseudo-harmonic mapping** (the
/// old doc claimed it was). Theirs is
/// `f(x) = (B-x)(1+A) / ((B-A)(1+x))`; this is a simpler reach-weighted
/// linear blend. It is also **unwired** — no caller in the repo; the live
/// runtime uses hard `action_bucket` thresholds instead (a known exploit
/// vector, see the 2026-10-02 competitiveness review §R6). Either wire a
/// correct pseudo-harmonic mapping at runtime or delete this.
///
/// Given two abstract actions (`lower` and `upper`) with their reach
/// probabilities under the opponent's current strategy, and an actual
/// (continuous) action that falls between them, compute the (lower, upper)
/// probabilities quantised to `u8` (0–255 scale where the two values sum to 255).
pub fn compute_translation(
    lower: f32,
    upper: f32,
    actual: f32,
    reach_lower: f32,
    reach_upper: f32,
) -> (u8, u8) {
    let denom = (upper - actual) * reach_lower + (actual - lower) * reach_upper;

    let p_lower: f32 = if denom.abs() < f32::EPSILON {
        // Degenerate case: fall back to uniform distribution
        0.5
    } else {
        let raw = 1.0 - ((actual - lower) * reach_upper) / denom;
        raw.clamp(0.0, 1.0)
    };

    let lower_q = (p_lower * 255.0).round() as u8;
    let upper_q = 255u8 - lower_q; // ensures exact sum of 255
    (lower_q, upper_q)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_sum_to_255((l, u): (u8, u8)) {
        assert_eq!(
            l as u16 + u as u16,
            255,
            "lower={l}, upper={u} does not sum to 255"
        );
    }

    #[test]
    fn given_example_from_task() {
        let (l, u) = compute_translation(0.33, 0.50, 0.42, 0.6, 0.4);
        assert_sum_to_255((l, u));
        let denom = (0.50 - 0.42) * 0.6 + (0.42 - 0.33) * 0.4;
        let p_lower = 1.0 - (0.09 * 0.4) / denom;
        let expected_lower_q = (p_lower * 255.0_f64).round() as u8;
        let diff = (l as i16 - expected_lower_q as i16).abs();
        assert!(diff <= 1, "expected lower ~{expected_lower_q}, got {l}");
        assert!(l > 128);
    }

    #[test]
    fn uniform_if_denom_zero() {
        let (l, u) = compute_translation(0.0, 1.0, 0.4, 0.0, 0.0);
        assert_sum_to_255((l, u));
        assert_eq!(l, 128);
        assert_eq!(u, 127);
    }

    #[test]
    fn denom_zero_when_both_reach_zero_even_with_nonzero_diff() {
        let (l, u) = compute_translation(0.2, 0.8, 0.5, 0.0, 0.0);
        assert_eq!(l, 128);
        assert_eq!(u, 127);
    }

    #[test]
    fn edge_actual_equals_lower() {
        let (l, u) = compute_translation(0.1, 0.5, 0.1, 0.3, 0.7);
        assert_sum_to_255((l, u));
        assert_eq!(l, 255);
        assert_eq!(u, 0);
    }

    #[test]
    fn edge_actual_equals_upper() {
        let (l, u) = compute_translation(0.1, 0.5, 0.5, 0.3, 0.7);
        assert_sum_to_255((l, u));
        assert_eq!(l, 0);
        assert_eq!(u, 255);
    }

    #[test]
    fn extreme_reach_probabilities() {
        let (l, u) = compute_translation(0.0, 1.0, 0.3, 1.0, 0.0);
        assert_eq!(l, 255);
        assert_eq!(u, 0);

        let (l, u) = compute_translation(0.0, 1.0, 0.7, 0.0, 1.0);
        assert_eq!(l, 0);
        assert_eq!(u, 255);
    }

    #[test]
    fn equal_reach_probabilities_and_symmetric_actual() {
        let (l, u) = compute_translation(0.0, 1.0, 0.5, 0.5, 0.5);
        assert_sum_to_255((l, u));
        let diff = (l as i16 - 128).abs();
        assert!(diff <= 1, "expected close to 128, got {l}");
    }

    #[test]
    fn large_numerical_stability() {
        let (l, u) = compute_translation(1000.0, 2000.0, 1500.0, 0.5, 0.5);
        assert_sum_to_255((l, u));
    }

    #[test]
    fn sum_to_255_always() {
        let test_cases = [
            (0.0, 1.0, 0.2, 0.3, 0.7),
            (0.33, 0.50, 0.42, 0.6, 0.4),
            (0.1, 0.9, 0.5, 0.2, 0.8),
            (0.0, 0.5, 0.25, 0.5, 0.5),
            (0.2, 0.8, 0.2, 0.0, 1.0),
            (0.2, 0.8, 0.8, 1.0, 0.0),
        ];
        for &(lo, hi, act, rl, ru) in &test_cases {
            assert_sum_to_255(compute_translation(lo, hi, act, rl, ru));
        }
    }

    #[test]
    fn rounding_does_not_exceed_byte_range() {
        let (l, u) = compute_translation(0.0, 1.0, 0.9999, 0.5, 0.5);
        assert_eq!(l as u16 + u as u16, 255);
    }

    #[test]
    fn no_panic_on_nan_input() {
        let _ = compute_translation(f32::NAN, 1.0, 0.5, 0.5, 0.5);
        let _ = compute_translation(0.0, f32::NAN, 0.5, 0.5, 0.5);
        let _ = compute_translation(0.0, 1.0, f32::NAN, 0.5, 0.5);
        let _ = compute_translation(0.0, 1.0, 0.5, f32::NAN, 0.5);
        let _ = compute_translation(0.0, 1.0, 0.5, 0.5, f32::NAN);
    }

    #[test]
    fn favoring_action_with_higher_reach() {
        let (l1, _) = compute_translation(0.0, 1.0, 0.5, 0.9, 0.1);
        let (l2, _) = compute_translation(0.0, 1.0, 0.5, 0.1, 0.9);
        assert!(l1 > l2);
    }

    #[test]
    fn reach_sum_less_than_one() {
        let (l, u) = compute_translation(0.0, 1.0, 0.3, 0.2, 0.0);
        assert_eq!(l, 255);
        assert_eq!(u, 0);
    }

    #[test]
    fn reach_probabilities_gt_one_still_works() {
        let (l, u) = compute_translation(0.0, 1.0, 0.5, 2.0, 3.0);
        assert_sum_to_255((l, u));
    }

    #[test]
    fn tiny_positive_denom_no_underflow() {
        let (l, u) = compute_translation(0.0, 1e-40, 0.5e-40, 1.0, 1.0);
        assert_sum_to_255((l, u));
    }

    #[test]
    fn exactly_one_probability_gets_all_mass() {
        let (l, u) = compute_translation(0.0, 1.0, 0.0, 1.0, 0.0);
        assert_eq!(l, 255);
        assert_eq!(u, 0);

        let (l, u) = compute_translation(0.0, 1.0, 1.0, 0.0, 1.0);
        assert_eq!(l, 0);
        assert_eq!(u, 255);
    }

    #[test]
    fn actual_slightly_above_upper_graceful() {
        let (l, u) = compute_translation(0.0, 0.5, 0.5000001, 0.4, 0.6);
        assert_sum_to_255((l, u));
    }

    #[test]
    fn p_lower_monotonically_decreases_with_actual() {
        let (l1, _) = compute_translation(0.2, 0.8, 0.3, 0.4, 0.6);
        let (l2, _) = compute_translation(0.2, 0.8, 0.5, 0.4, 0.6);
        assert!(l1 > l2, "p_lower should decrease as actual moves right");
    }

    #[test]
    fn all_zero_inputs_returns_uniform() {
        let (l, u) = compute_translation(0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(l, 128);
        assert_eq!(u, 127);
    }

    #[test]
    fn result_is_deterministic() {
        let a = compute_translation(0.1, 0.9, 0.5, 0.3, 0.7);
        let b = compute_translation(0.1, 0.9, 0.5, 0.3, 0.7);
        assert_eq!(a, b);
    }
    #[test]
    fn equal_reach_gives_linear_interpolation() {
        // With equal reach probabilities, formula reduces to linear interpolation
        let (l, u) = compute_translation(0.2, 0.8, 0.5, 0.4, 0.4);
        assert_sum_to_255((l, u));
        let expected = (0.8 - 0.5) / (0.8 - 0.2);
        let expected_q = (expected * 255.0_f64).round() as u8;
        let diff = (l as i16 - expected_q as i16).abs();
        assert!(diff <= 1, "expected close to {expected_q}, got {l}");
    }

    #[test]
    fn actual_outside_bounds_clamps() {
        // Actual below lower: should favour lower action (p_lower ≈ 1.0)
        let (l, u) = compute_translation(0.2, 0.8, 0.1, 0.5, 0.5);
        assert_eq!(l, 255);
        assert_eq!(u, 0);

        // Actual above upper: should favour upper action (p_lower ≈ 0.0)
        let (l, u) = compute_translation(0.2, 0.8, 0.9, 0.5, 0.5);
        assert_eq!(l, 0);
        assert_eq!(u, 255);
    }

    #[test]
    fn negative_reach_probabilities_handled() {
        // Function should not panic and sum must stay 255
        let (l, u) = compute_translation(0.0, 1.0, 0.5, -0.5, 0.5);
        assert_sum_to_255((l, u));
    }

    #[test]
    fn negative_action_values_handled() {
        let (l, u) = compute_translation(-0.5, 0.5, 0.0, 0.3, 0.7);
        assert_sum_to_255((l, u));
    }
}
