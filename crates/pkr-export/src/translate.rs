/// Pseudo-harmonic mapping for off-tree action translation
/// as per Ganzfried & Sandholm (2013).
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

    const EPS: f32 = 1e-5;

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
        let expected_lower_q = (p_lower as f64 * 255.0).round() as u8;
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
            let res = compute_translation(lo, hi, act, rl, ru);
            assert_sum_to_255(res);
        }
    }

    #[test]
    fn rounding_does_not_exceed_255() {
        let (l, u) = compute_translation(0.0, 1.0, 0.9999, 0.5, 0.5);
        assert!(l <= 255);
        assert!(u <= 255);
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
}
