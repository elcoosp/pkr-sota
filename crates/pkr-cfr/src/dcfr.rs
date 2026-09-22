/// DCFR (Discounted CFR) update logic.
///
/// Implements the discounted-regret and discounted-strategy-sum scheme from
/// Brown & Sandholm (2019), "Superhuman AI using the DCFR algorithm".
///
/// Parameters (canonical defaults):
///   α = 1.5  — positive-regret discount weight
///   β = 0.0  — negative-regret discount weight (no discount)
///   γ = 2.0  — strategy-sum discount weight
///   τ = 1000 — warmup iterations before discounting kicks in
///
/// For t < τ: discount factors are 1.0 (no discounting).
/// For t ≥ τ: regret is multiplied by (t/τ)^α for positive, (t/τ)^β for negative.
///             strategy-sum is multiplied by (t/τ)^γ.
///
/// The GPU shader mirrors this exactly (see gpu.rs SHADER).
/// Both CPU and GPU use the same SCALE = 1000.0 fixed-point encoding.

pub const ALPHA: f32 = 1.5;
pub const BETA: f32 = 0.0;
pub const GAMMA: f32 = 2.0;
pub const TAU: u32 = 1000;

/// Discount factor for regret at iteration t, given exponent p.
/// For t < TAU, returns 1.0 (no discounting during warmup).
/// For t >= TAU: returns (t/τ)^p — the multiplicative weight
/// from Brown & Sandholm 2019 DCFR.
/// Which DCFR discount formula to use. Selectable so we can measure
/// convergence on small games (Kuhn) and pick empirically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscountMode {
    /// No discount. Equivalent to vanilla CFR.
    None,
    /// Brown & Sandholm 2019 canonical DCFR: t^p / (t^p + 1).
    /// Factor is in [0.5, 1), so regrets decay slowly. This is the
    /// formula the paper specifies.
    CanonicalDcfr,
    /// Current pkr-sota code: (t/τ)^p. Growth-based, not decay-based.
    /// At t=1e6 with α=1.5 this is ~3e4, so positive regrets are
    /// amplified every update. Kept as the default until the Kuhn
    /// experiment says otherwise.
    RatioPower,
}

/// Whether to use the PCFR+ momentum term in the regret update.
/// Off = plain CFR: regret_new = discount(regret_old) + delta.
/// On  = PCFR+ (Farina et al. 2021): regret_new = discount(regret_old)
///       + [(1-gamma)*prev_momentum + gamma*delta].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MomentumMode {
    Off,
    On,
}

#[inline(always)]
pub fn discount_factor_mode(t: f32, p: f32, mode: DiscountMode) -> f32 {
    if t < TAU as f32 {
        return 1.0;
    }
    match mode {
        DiscountMode::None => 1.0,
        DiscountMode::CanonicalDcfr => {
            let tp = t.powf(p);
            tp / (tp + 1.0)
        }
        DiscountMode::RatioPower => {
            let ratio = t / TAU as f32;
            ratio.powf(p)
        }
    }
}

#[inline(always)]
pub fn discount_factor(t: f32, p: f32) -> f32 {
    discount_factor_mode(t, p, DiscountMode::CanonicalDcfr)
}

/// PCFR+ momentum update (Farina, Kroer, Sandholm 2021), composed with DCFR.
/// Returns (new_regret, new_momentum).
///
/// This is the CPU fallback path — the GPU shader in gpu.rs mirrors this.
#[inline(always)]
pub fn update_regret_full(
    current: f32,
    prev_momentum: f32,
    iteration: u32,
    delta: f32,
    discount: DiscountMode,
    momentum: MomentumMode,
) -> (f32, f32) {
    let t = iteration as f32;
    if t == 0.0 {
        return (delta, delta);
    }

    let predicted_delta = match momentum {
        MomentumMode::On => {
            let gamma = 1.0 / (t + 1.0).sqrt();
            (1.0 - gamma) * prev_momentum + gamma * delta
        }
        MomentumMode::Off => delta,
    };

    let r_pos = current.max(0.0);
    let r_neg = current.min(0.0);

    let w_pos = discount_factor_mode(t, ALPHA, discount);
    let w_neg = discount_factor_mode(t, BETA, discount);

    let discounted_regret = w_pos * r_pos + w_neg * r_neg;
    let new_regret = (discounted_regret + predicted_delta).max(0.0);

    (new_regret, predicted_delta)
}

/// Backwards-compatible wrapper: canonical discount + PCFR+ momentum on.
pub fn update_regret_pfr_plus_mode(
    current: f32,
    prev_momentum: f32,
    iteration: u32,
    delta: f32,
    mode: DiscountMode,
) -> (f32, f32) {
    update_regret_full(
        current,
        prev_momentum,
        iteration,
        delta,
        mode,
        MomentumMode::On,
    )
}

#[inline(always)]
pub fn update_regret_pfr_plus(
    current: f32,
    prev_momentum: f32,
    iteration: u32,
    delta: f32,
) -> (f32, f32) {
    // CanonicalDcfr is the default. RatioPower (the previous default)
    // multiplies regrets by (t/τ)^p every update, which overflows f32
    // around iteration 3000 for any infoset visited dozens of times.
    // Demonstrated in crates/pkr-testgames kuhn-experiment: ratio-power
    // produces NaN by t=3000, canonical does not and converges.
    update_regret_pfr_plus_mode(
        current,
        prev_momentum,
        iteration,
        delta,
        DiscountMode::CanonicalDcfr,
    )
}

/// Standard DCFR update (without momentum) for backward compatibility.
pub fn update_regret(current: f32, iteration: u32, delta: f32) -> f32 {
    let t = iteration as f32;
    if t == 0.0 {
        return current + delta;
    }

    let r_pos = current.max(0.0);
    let r_neg = current.min(0.0);

    let w_pos = discount_factor(t, ALPHA);
    let w_neg = discount_factor(t, BETA);

    let discounted_regret = w_pos * r_pos + w_neg * r_neg + delta;
    discounted_regret.max(0.0)
}

/// Strategy-sum discount factor for averaging.
/// Uses γ=2 with τ=1000 warmup.
#[inline(always)]
pub fn strategy_sum_discount_factor(t: f32) -> f32 {
    if t < TAU as f32 {
        return 1.0;
    }
    let ratio = t / TAU as f32;
    let numerator = ratio.powf(GAMMA);
    numerator / (numerator + 1.0)
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
        // t=2 < TAU=1000, so discount_factor = 1.0 (no discount)
        // update_regret = max(0, 1.0 * 10.0 + 5.0) = 15.0
        let r = update_regret(10.0, 2, 5.0);
        assert_eq!(r, 15.0);
    }

    #[test]
    fn warmup_iterations_no_discount() {
        // t < τ: discount factor should be 1.0
        assert!((discount_factor(1.0, ALPHA) - 1.0).abs() < 1e-6);
        assert!((discount_factor(500.0, ALPHA) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn discount_factor_canonical_is_bounded_and_monotonic() {
        // Canonical DCFR: w = t^p / (t^p + 1). Bounded in [0.5, 1), and
        // increases monotonically toward 1 as t -> infinity. For any
        // finite t, w < 1.0, so regrets are damped, not amplified.
        let f1 = discount_factor(1001.0, ALPHA);
        let f2 = discount_factor(2000.0, ALPHA);
        let f3 = discount_factor(5000.0, ALPHA);

        assert!(
            f1 >= 0.5 && f1 < 1.0,
            "at t=1001, discount_factor must be in [0.5, 1), got {f1}"
        );
        assert!(
            f1 <= f2 && f2 <= f3,
            "discount should be monotonically increasing with t:              f1={f1}, f2={f2}, f3={f3}"
        );
        assert!(
            f3 < 1.0,
            "discount must remain < 1.0 for finite t, got {f3}"
        );
    }

    #[test]
    fn strategy_sum_discount_warmup() {
        assert!((strategy_sum_discount_factor(500.0) - 1.0).abs() < 1e-6);
        // For t > τ: w = (t/τ)^γ / ((t/τ)^γ + 1), approaches 1 from below
        assert!(strategy_sum_discount_factor(5000.0) < 1.0);
    }

    #[test]
    fn discount_factor_no_nan() {
        let f = discount_factor(1e6, ALPHA);
        assert!(f.is_finite());
        assert!(f >= 0.0);
    }

    #[test]
    fn beta_zero_means_fast_negative_discount() {
        // Canonical DCFR with β=0: w_neg = t^0 / (t^0 + 1) = 1/2.
        // Negative regrets are halved every step, so failures are
        // forgotten quickly. This is the paper's intent for β<α.
        let f = discount_factor(2000.0, BETA);
        assert!((f - 0.5).abs() < 0.01, "expected 0.5, got {}", f);
    }
}
