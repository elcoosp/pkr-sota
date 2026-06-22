/// Discounted CFR regret update using f32.
/// Applies factor = t^alpha / (t^alpha + 1) to current regret and adds delta.
/// alpha = 1.5 for positive delta, 0.0 for negative delta.
pub fn update_regret(current: f32, iteration: u32, delta: f32, is_positive: bool) -> f32 {
    let t = iteration as f32;
    let factor = if t == 0.0 {
        0.0
    } else {
        let alpha = if is_positive { 1.5 } else { 0.0 };
        let pow = t.powf(alpha);
        pow / (pow + 1.0)
    };
    current * factor + delta
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discount_factor(t: u32, alpha: f32) -> f32 {
        if t == 0 { return 0.0; }
        let t_f = t as f32;
        let pow = t_f.powf(alpha);
        pow / (pow + 1.0)
    }

    #[test]
    fn positive_delta_discounted() {
        let r = update_regret(10.0, 2, 5.0, true);
        let expected = 10.0 * discount_factor(2, 1.5) + 5.0;
        assert!((r - expected).abs() < 1e-5);
    }

    #[test]
    fn negative_delta_discounted() {
        let r = update_regret(-5.0, 2, -3.0, false);
        let expected = -5.0 * discount_factor(2, 0.0) + -3.0;
        assert!((r - expected).abs() < 1e-5);
    }

    #[test]
    fn zero_iteration_factor_zero() {
        assert_eq!(update_regret(10.0, 0, 5.0, true), 5.0);
        assert_eq!(update_regret(10.0, 0, -5.0, false), -5.0);
    }
}
