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
}
