/// DCFR (Discounted CFR) math helper.
///
/// Applies the discount factor to the current cumulative regret and adds
/// instantaneous delta, then clamps the result back into the `u8` range.
///
/// The discount factor depends on the `is_positive` flag:
/// - true  → α = 1.5  → factor = t^1.5 / (t^1.5 + 1)
/// - false → α = 0.0  → factor = t^0   / (t^0   + 1)
///
/// The current regret is stored as an unsigned byte with midpoint 128
/// representing zero.  The offset (current - 128) is discounted, added to
/// `delta`, and finally re-centred and clamped to [0, 255].
pub fn update_regret(current: u8, iteration: u32, delta: f32, is_positive: bool) -> u8 {
    let offset = current as f32 - 128.0;
    let t = iteration as f32;

    let factor = if t == 0.0 {
        0.0 // limit of t^a/(t^a+1) as t→0
    } else {
        let alpha = if is_positive { 1.5 } else { 0.0 };
        let pow = t.powf(alpha);
        pow / (pow + 1.0)
    };

    let discounted_offset = offset * factor;
    let new_offset = discounted_offset + delta;
    let new_val = 128.0 + new_offset;

    // Clamp to u8 range and round to nearest integer
    (new_val.round() as i32).clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── helper to compute expected discount factor ─────────
    fn discount_factor(t: u32, alpha: f32) -> f32 {
        if t == 0 {
            return 0.0;
        }
        let t_f = t as f32;
        let pow = t_f.powf(alpha);
        pow / (pow + 1.0)
    }

    // Original tests from green phase
    #[test]
    fn positive_regret_discounted_by_alpha_1_5() {
        let current = 200u8; // offset +72
        let t = 2;
        let factor = discount_factor(t, 1.5);
        let expected_offset = 72.0 * factor;
        let expected = (128.0 + expected_offset).round() as u8;
        let result = update_regret(current, t, 0.0, true);
        assert_eq!(result, expected, "positive discount for t=2");
    }

    #[test]
    fn negative_regret_discounted_by_alpha_0() {
        let current = 56u8; // offset -72
        let t = 2;
        let factor = discount_factor(t, 0.0); // 0.5
        let expected_offset = -72.0 * factor;
        let expected = (128.0 + expected_offset).round() as u8;
        let result = update_regret(current, t, 0.0, false);
        assert_eq!(result, expected, "negative discount for t=2");
    }

    #[test]
    fn t_zero_discount_factor_is_zero() {
        let current = 200u8;
        let result = update_regret(current, 0, 10.0, true);
        assert_eq!(result, 138);
        let result_neg = update_regret(56u8, 0, -10.0, false);
        assert_eq!(result_neg, 118);
    }

    #[test]
    fn t_one_factors_are_equal() {
        let current = 200u8;
        let r_pos = update_regret(current, 1, 0.0, true);
        let r_neg = update_regret(current, 1, 0.0, false);
        assert_eq!(r_pos, r_neg);
        assert_eq!(r_pos, 164);
    }

    #[test]
    fn delta_added_after_discount() {
        let current = 128u8;
        let r = update_regret(current, 2, 15.0, true);
        assert_eq!(r, 143);
    }

    #[test]
    fn result_clamps_to_u8_range() {
        let r = update_regret(255u8, 2, 200.0, true);
        assert_eq!(r, 255);
        let r2 = update_regret(0u8, 2, -200.0, false);
        assert_eq!(r2, 0);
    }

    #[test]
    fn flag_controls_discount_even_on_same_offset() {
        let current = 200u8;
        let t = 2;
        let r_pos = update_regret(current, t, 0.0, true);
        let r_neg = update_regret(current, t, 0.0, false);
        assert_ne!(r_pos, r_neg);
        assert!(r_pos > r_neg);
    }

    #[test]
    fn no_nan_panic() {
        let _ = update_regret(128, 0, 0.0, true);
        let _ = update_regret(128, 0, 0.0, false);
        let _ = update_regret(128, 100, 0.0, true);
        let _ = update_regret(128, 100, 0.0, false);
    }

    // ── Additional tests for W2-T1 (more coverage) ───────

    #[test]
    fn large_t_positive_discount_approaches_one() {
        // For t=1000, factor ≈ 0.999... so offset is almost unchanged
        let current = 200u8; // offset +72
        let t = 1000;
        let factor = discount_factor(t, 1.5);
        assert!(factor > 0.99, "factor should be > 0.99, got {factor}");
        let result = update_regret(current, t, 0.0, true);
        let expected_offset = 72.0 * factor;
        let expected = (128.0 + expected_offset).round() as u8;
        // Allow rounding difference of 1 due to f32
        assert!(
            (result as i16 - expected as i16).abs() <= 1,
            "result {result} vs expected {expected}"
        );
    }

    #[test]
    fn negative_discount_constant_after_t0() {
        // For alpha=0, factor = 0.5 for any t>0
        for t in 1..=10 {
            let factor = discount_factor(t, 0.0);
            assert!((factor - 0.5).abs() < 1e-6, "t={t}: factor={factor}");
        }
        // Also test the actual function gives same discount regardless of t (for negative flag)
        let current = 100u8; // offset -28
        let r1 = update_regret(current, 1, 0.0, false);
        let r2 = update_regret(current, 5, 0.0, false);
        let r3 = update_regret(current, 100, 0.0, false);
        assert_eq!(r1, r2);
        assert_eq!(r2, r3);
    }

    #[test]
    fn monotonic_positive_factor() {
        // factor should increase with t
        let mut prev = discount_factor(1, 1.5);
        for t in 2..=10 {
            let cur = discount_factor(t, 1.5);
            assert!(cur > prev, "factor not monotonic at t={t}: {cur} <= {prev}");
            prev = cur;
        }
    }

    #[test]
    fn clamping_at_lower_bound() {
        // start at 0 (offset -128), large negative delta should stay 0
        let r = update_regret(0, 5, -1000.0, false);
        assert_eq!(r, 0);
        // with no delta, offset -128 * factor (0.5) = -64 → 128-64=64
        let r2 = update_regret(0, 5, 0.0, false);
        assert_eq!(r2, 64);
    }

    #[test]
    fn clamping_at_upper_bound() {
        let r = update_regret(255, 5, 1000.0, true);
        assert_eq!(r, 255);
        // offset 127 * factor (~0.96) + 0 = ~122 → 128+122=250
        let r2 = update_regret(255, 5, 0.0, true);
        // compute expected manually
        let factor = discount_factor(5, 1.5);
        let expected = (128.0 + 127.0 * factor).round() as u8;
        assert_eq!(r2, expected);
    }

    #[test]
    fn delta_zero_preserves_discounted_offset() {
        let current = 180u8; // offset +52
        let t = 3;
        let factor = discount_factor(t, 1.5);
        let expected = (128.0 + 52.0 * factor).round() as u8;
        let r = update_regret(current, t, 0.0, true);
        assert_eq!(r, expected);
        // For negative flag
        let factor_neg = discount_factor(t, 0.0);
        let expected_neg = (128.0 + 52.0 * factor_neg).round() as u8;
        let r_neg = update_regret(current, t, 0.0, false);
        assert_eq!(r_neg, expected_neg);
    }

    #[test]
    fn delta_sign_flips_offset() {
        // positive delta increases value, negative decreases
        let r_up = update_regret(128, 2, 10.0, true);
        let r_down = update_regret(128, 2, -10.0, true);
        assert_eq!(r_up, 138);
        assert_eq!(r_down, 118);
    }

    #[test]
    fn mixed_positive_negative_with_delta() {
        // Positive flag, delta = -20: offset 72 * factor ≈ 72*0.7387 = 53.2
        // plus delta = 33.2 → 128+33.2=161.2 → round 161
        let result = update_regret(200, 2, -20.0, true);
        assert_eq!(result, 161);
        // Negative flag, delta = +30: offset 72 * 0.5 = 36 +30 = 66 → 128+66=194
        let result_neg = update_regret(200, 2, 30.0, false);
        assert_eq!(result_neg, 194);
    }

    #[test]
    fn extreme_iteration_handling() {
        // t=u32::MAX: large exponent may cause overflow? powf handles it gracefully
        let t_max = u32::MAX;
        let result = update_regret(128, t_max, 0.0, true);
        // factor should be 1.0, result remains 128
        assert_eq!(result, 128);
        let result_neg = update_regret(128, t_max, 0.0, false);
        assert_eq!(result_neg, 128);
    }

    #[test]
    fn result_always_in_u8_range() {
        // brute force a few values to ensure no panics or overflow
        let values = [0u8, 1, 100, 128, 200, 254, 255];
        let deltas = [-1000.0, -1.0, 0.0, 1.0, 1000.0];
        for &cur in &values {
            for &delta in &deltas {
                for &is_pos in &[true, false] {
                    // Calling the function is enough to verify it doesn't panic;
                    // the return type is u8 so it is automatically in range.
                    let _ = update_regret(cur, 5, delta, is_pos);
                }
            }
        }
    }

    #[test]
    fn idempotent_if_no_delta_and_t_large() {
        // For large t, factor ≈ 1, so offset unchanged
        let cur = 150u8;
        let r = update_regret(cur, 10_000, 0.0, true);
        assert_eq!(r, cur);
        // negative flag factor is 0.5, so offset halves, not idempotent
        let r_neg = update_regret(cur, 10_000, 0.0, false);
        // offset 22 * 0.5 = 11 -> 139
        assert_eq!(r_neg, 139);
    }

    #[test]
    fn iteration_zero_ignores_current_value() {
        // t=0 => factor=0, so only delta matters
        let cur = 200u8;
        let r = update_regret(cur, 0, -50.0, false);
        assert_eq!(r, 78); // 128 -50 = 78
        let r2 = update_regret(0, 0, 200.0, true);
        assert_eq!(r2, 255); // 128+200=328 clamped to 255
    }
}
