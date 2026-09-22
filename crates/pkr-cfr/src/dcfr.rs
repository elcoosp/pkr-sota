//! Discounted CFR regret/strategy update.
//!
//! Implements the discounted-regret and discounted-strategy-sum scheme
//! from Brown & Sandholm (2019), "Superhuman AI using the DCFR algorithm".
//!
//! Discount semantics: for t < TAU the factor is 1.0 (warmup). For
//! t >= TAU the factor is t^p / (t^p + 1), which is bounded in [0.5, 1)
//! and always < 1 for finite t. Positive regrets (p = ALPHA = 1.5) decay
//! slowly; negative regrets (p = BETA = 0.0) decay fast.
//!
//! The previous `RatioPower` formula `(t/τ)^p` was removed after the
//! Kuhn harness proved it overflows f32 at t≈3000 for any infoset
//! visited a few dozen times, producing NaN and silently corrupting
//! training. It is not available as an option.
//!
//! This module is the ONLY place regret updates are computed on the CPU
//! path; the WGSL shader in gpu.rs mirrors the same math. Both use the
//! same SCALE = 1000.0 fixed-point encoding.

pub const ALPHA: f32 = 1.5;
pub const BETA: f32 = 0.0;
pub const GAMMA: f32 = 2.0;
pub const TAU: u32 = 1000;

/// Which DCFR discount formula to use.
///
/// Only bounded, numerical-safe options are available. Adding a new
/// variant requires a run of `crates/pkr-testgames --bin kuhn-experiment`
/// proving it does not produce non-finite regrets and still converges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscountMode {
    /// No discount. Equivalent to vanilla CFR.
    None,
    /// Brown & Sandholm 2019 canonical DCFR: t^p / (t^p + 1).
    /// Bounded in [0.5, 1), cannot overflow for any finite t.
    CanonicalDcfr,
}

impl DiscountMode {
    /// The formula used in production. Verified via the Kuhn harness:
    /// converges to exploitability 1.24e-3 at t=3M with bounded regrets.
    pub const PRODUCTION: DiscountMode = DiscountMode::CanonicalDcfr;
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

/// Bounded discount factor at iteration t for exponent p.
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
    }
}

/// Discount factor using the production default. Kept for tests and for
/// code that does not need to be mode-aware.
#[inline(always)]
pub fn discount_factor(t: f32, p: f32) -> f32 {
    discount_factor_mode(t, p, DiscountMode::PRODUCTION)
}

/// Full regret update with selectable discount and momentum.
/// Returns (new_regret, new_momentum).
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

/// Production update: canonical discount, PCFR+ momentum on. This is
/// what `flush_cpu_batch` calls.
#[inline(always)]
pub fn update_regret_pfr_plus(
    current: f32,
    prev_momentum: f32,
    iteration: u32,
    delta: f32,
) -> (f32, f32) {
    update_regret_full(
        current,
        prev_momentum,
        iteration,
        delta,
        DiscountMode::PRODUCTION,
        MomentumMode::On,
    )
}

/// Strategy-sum discount factor. The production strategy accumulator
/// uses the identity multiplier (no discount); this is retained for
/// callers that want to experiment.
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
        // t=2 < TAU=1000, so discount factor = 1.0 (no discount).
        // regret = max(0, 1.0 * 10.0 + 5.0) = 15.0.
        let (r, _m) =
            update_regret_full(10.0, 0.0, 2, 5.0, DiscountMode::CanonicalDcfr, MomentumMode::Off);
        assert_eq!(r, 15.0);
    }

    #[test]
    fn warmup_iterations_no_discount() {
        assert!((discount_factor(1.0, ALPHA) - 1.0).abs() < 1e-6);
        assert!((discount_factor(500.0, ALPHA) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn discount_factor_canonical_is_bounded_and_monotonic() {
        // Canonical DCFR: w = t^p / (t^p + 1). Bounded in [0.5, 1).
        let f1 = discount_factor(1001.0, ALPHA);
        let f2 = discount_factor(2000.0, ALPHA);
        let f3 = discount_factor(5000.0, ALPHA);

        assert!((0.5..1.0).contains(&f1), "f1 out of range: {f1}");
        assert!(f1 <= f2 && f2 <= f3, "not monotonic: {f1} {f2} {f3}");
        assert!(f3 < 1.0, "must stay < 1: {f3}");
    }

    #[test]
    fn canonical_never_overflows_at_large_t() {
        // The removed RatioPower formula produced inf here. Canonical
        // cannot: t^p / (t^p + 1) in exact arithmetic is in [0.5, 1).
        //
        // In f32 it saturates to exactly 1.0 once t^p > 2^23 ~ 8.4e6,
        // because the + 1.0 is below the float's epsilon at that
        // magnitude. This is expected and is why canonical behaves
        // identically to no-discount at large t.
        for &t in &[1e3_f32, 1e6, 1e9, 1e12] {
            let f = discount_factor(t, ALPHA);
            assert!(f.is_finite(), "t={t}: not finite");
            assert!(f >= 0.5, "t={t}: below 0.5");
            assert!(f <= 1.0, "t={t}: above 1.0");
        }
    }

    #[test]
    fn canonical_strictly_below_one_in_transitional_range() {
        // For t in the 1e3 - 1e4 range, t^p is small enough that + 1.0
        // still registers, so the discount is strictly < 1.0. This is
        // the only window where canonical differs from no-discount in
        // f32.
        let f = discount_factor(2000.0, ALPHA);
        assert!(
            f < 1.0,
            "transitional range must be strictly < 1.0, got {f}"
        );
        assert!(f > 0.99, "transitional range should be close to 1.0, got {f}");
    }

    #[test]
    fn strategy_sum_discount_warmup() {
        assert!((strategy_sum_discount_factor(500.0) - 1.0).abs() < 1e-6);
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
        // Canonical DCFR with β=0: w_neg = t^0 / (t^0 + 1) = 0.5.
        let f = discount_factor(2000.0, BETA);
        assert!((f - 0.5).abs() < 0.01, "expected 0.5, got {f}");
    }

    #[test]
    fn production_default_is_canonical() {
        assert_eq!(DiscountMode::PRODUCTION, DiscountMode::CanonicalDcfr);
    }
}
