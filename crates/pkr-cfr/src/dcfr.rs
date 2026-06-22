/// PCFR+ momentum update (Farina, Kroer, Sandholm 2021).
/// Returns (new_regret, new_momentum).
pub fn update_regret_pfr_plus(
    current: f32,
    prev_momentum: f32,
    iteration: u32,
    delta: f32,
) -> (f32, f32) {
    let t = iteration as f32;
    if t == 0.0 {
        return (delta, delta);
    }

    // Momentum decay factor
    let gamma = 1.0 / (t + 1.0).sqrt();
    let predicted_delta = (1.0 - gamma) * prev_momentum + gamma * delta;

    // DCFR discounting (Brown & Sandholm 2019) on current regret
    let r_pos = current.max(0.0);
    let r_neg = current.min(0.0);
    let alpha = 1.5f32;
    let beta = 0.0f32;
    let t_a = t.powf(alpha);
    let t_b = t.powf(beta);
    let w_pos = t_a / (t_a + 1.0);
    let w_neg = t_b / (t_b + 1.0);
    let discounted_regret = w_pos * r_pos + w_neg * r_neg;

    // PCFR+ alternation: max(0, discounted + predicted)
    let new_regret = (discounted_regret + predicted_delta).max(0.0);

    (new_regret, predicted_delta)
}

/// Standard DCFR update (without momentum) for backward compatibility.
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
    fn pcfr_plus_basic() {
        let (r, m) = update_regret_pfr_plus(0.0, 0.0, 1, 5.0);
        assert!(r > 0.0);
        assert!(m > 0.0);
    }

    #[test]
    fn standard_dcfr_no_momentum() {
        let r = update_regret(10.0, 2, 5.0);
        assert!(r > 5.0);
        assert!(r < 15.0);
    }
}
