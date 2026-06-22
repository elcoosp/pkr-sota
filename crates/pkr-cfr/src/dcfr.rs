/// Discounted CFR regret update (Brown & Sandholm 2019).
/// Separates current regret into positive and negative parts,
/// applies discount factors, then adds delta.
pub fn update_regret(current: f32, iteration: u32, delta: f32) -> f32 {
    let t = iteration as f32;
    if t == 0.0 {
        return current + delta;
    }
    let r_pos = current.max(0.0);
    let r_neg = current.min(0.0);
    let alpha = 1.5f32;
    let beta = 0.0f32;
    let t_a = t.powf(alpha);
    let t_b = t.powf(beta);
    let w_pos = t_a / (t_a + 1.0);
    let w_neg = t_b / (t_b + 1.0);
    w_pos * r_pos + w_neg * r_neg + delta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_discount_preserves_sign() {
        let r = update_regret(10.0, 2, 5.0);
        assert!(r > 5.0);
        assert!(r < 15.0);
    }

    #[test]
    fn negative_discount_shrinks() {
        let r = update_regret(-10.0, 2, -3.0);
        assert!(r < -3.0);
        assert!(r > -13.0);
    }

    #[test]
    fn zero_iteration_no_discount() {
        assert_eq!(update_regret(10.0, 0, 5.0), 15.0);
        assert_eq!(update_regret(-10.0, 0, -5.0), -15.0);
    }
}
