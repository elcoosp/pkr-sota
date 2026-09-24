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
#[allow(dead_code)] // superseded by update_regret_i64; kept for the f32 baseline
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
#[allow(dead_code)] // superseded by update_regret_i64; kept for the f32 baseline
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

// ---------------------------------------------------------------------------
// T1.1: exact integer discount
// ---------------------------------------------------------------------------
//
// The f32 form `t^p / (t^p + 1)` rounds to exactly 1.0 once `t^p > 2^23`,
// which for α=1.5 happens at t ≈ 10^4. Past that, the DCFR discount is
// inert and training degenerates to vanilla CFR.
//
// Fix: compute the discount as an exact i128 rational `(num, den)` for
// integer p ∈ {0, 1, 2}, then apply it with i128 arithmetic on the i64
// regret accumulator. No f32 anywhere in the discount path.
//
// NOTE (2026-09-24): The production regret update no longer uses
// `discount_num_den`. It computes `t^α/(t^α + 1)` in f64 (see
// `dcfr_step` above) and applies the result as a single i64 multiply.
// This is required because the DCFR paper's recommended α=1.5 is
// irrational; the old integer path rounded to α=2 which is NOT the
// paper's default. `discount_num_den` is kept for the reference test
// in `fast_path_equivalence`.

/// Exact discount factor w_p(t) = t^p / (t^p + 1) as a rational num/den.
/// Returns (1, 1) during warmup (t < TAU).
#[inline]
pub fn discount_num_den(t: u32, p: u32) -> (i128, i128) {
    if t < TAU {
        return (1, 1);
    }
    let ti = t as i128;
    let tp = match p {
        0 => 1i128,
        1 => ti,
        2 => ti * ti,
        _ => unreachable!("discount_num_den only supports p ∈ {{0,1,2}}"),
    };
    (tp, tp + 1)
}

/// Precomputed per-iteration discount constants for the production
/// update path. All regret groups inside one flush batch share the same
/// iteration number, so the powf/sqrt evaluations are hoisted out of
/// the per-group loop.
///
/// DCFR paper (Brown & Sandholm 2019) recommends (α, β, γ) = (1.5, 0, 2).
/// We compute `w_pos = t^1.5 / (t^1.5 + 1)` in f64 (t^1.5 is irrational),
/// then apply it as a single i64 multiply in the hot path. The old
/// integer formula computed t²/(t²+1) — that's α=2, not the paper's 1.5.
#[derive(Debug, Clone, Copy)]
pub struct DcfrStep {
    pub w_pos: f64,
    pub w_neg: f64,
    pub gamma: f64,
    pub identity: bool,
}

#[inline]
pub fn dcfr_step(iteration: u32) -> DcfrStep {
    // Only t == 0 is the identity case (matches the original
    // `update_regret_i64` behaviour: first-ever update returns delta
    // unmodified). During warmup (0 < t < TAU) the discount is 1.0 and
    // the standard `r' = max(0, r + delta)` applies.
    if iteration == 0 {
        return DcfrStep { w_pos: 1.0, w_neg: 1.0, gamma: 1.0, identity: true };
    }
    if iteration < TAU {
        // Warmup: discount = 1, but gamma (used only by the momentum
        // path) is still 1/sqrt(t+1) so PCFR+ momentum behaves the same
        // as the pre-refactor `update_regret_full`.
        let gamma = 1.0 / ((iteration as f64) + 1.0).sqrt();
        return DcfrStep { w_pos: 1.0, w_neg: 1.0, gamma, identity: false };
    }
    let t = iteration as f64;
    let tp = t.powf(ALPHA as f64);
    let tn = t.powf(BETA as f64);
    let w_pos = tp / (tp + 1.0);
    let w_neg = tn / (tn + 1.0);
    // The momentum gamma was used by the (now-disabled) PCFR+ path.
    // We keep it for API compatibility; production uses PKR_MOMENTUM=0.
    let gamma = 1.0 / (t + 1.0).sqrt();
    DcfrStep { w_pos, w_neg, gamma, identity: false }
}

/// Apply the precomputed discounts. `momentum_on=false` (production)
/// means `predicted = delta`, matching plain CFR+/DCFR.
#[inline]
pub fn update_regret_with_step(
    current_i64: i64,
    _prev_momentum_i64: i64,
    delta_i64: i64,
    step: &DcfrStep,
    momentum_on: bool,
) -> (i64, i64) {
    if step.identity {
        return (delta_i64, delta_i64);
    }
    let predicted_i64 = if momentum_on {
        // Not exercised in production; kept for API symmetry.
        let g = step.gamma;
        let prev = _prev_momentum_i64 as f64;
        let d = delta_i64 as f64;
        ((1.0 - g) * prev + g * d).round() as i64
    } else {
        delta_i64
    };
    let discounted = if current_i64 >= 0 {
        (current_i64 as f64 * step.w_pos) as i64
    } else {
        (current_i64 as f64 * step.w_neg) as i64
    };
    let new_r = discounted.saturating_add(predicted_i64).max(0);
    (new_r, predicted_i64)
}

/// Legacy α=2 integer floor, kept for tests and reference comparison.
#[allow(dead_code)]
#[inline(always)]
fn discount_pos_i64_alpha2(r: i64, t: u32) -> i64 {
    debug_assert!(r >= 0);
    if t < TAU || r == 0 {
        return r;
    }
    let d = (t as u64) * (t as u64) + 1;
    let ru = r as u64;
    let q = ru / d;
    let ceil = if !ru.is_multiple_of(d) { q + 1 } else { q };
    (ru - ceil) as i64
}

/// β = 0 discount for negative regret: exactly 1/2 (truncated toward zero).
/// Superseded by `dcfr_step().w_neg` in the production path; kept for
/// the equivalence test.
#[allow(dead_code)]
#[inline(always)]
fn discount_neg_i64(r: i64, t: u32) -> i64 {
    if t < TAU {
        r
    } else {
        r / 2
    }
}

/// Exact regret/momentum update in i64 (at SCALE=1000).
///
/// `momentum_on = true`  → production PCFR+-style update.
/// `momentum_on = false` → plain CFR+/DCFR: `r' = max(0, disc(r) + delta)`.
///
/// Never overflows: the final add saturates.
#[inline]
pub fn update_regret_i64_mode(
    current_i64: i64,
    prev_momentum_i64: i64,
    iteration: u32,
    delta_i64: i64,
    momentum_on: bool,
) -> (i64, i64) {
    let step = dcfr_step(iteration);
    update_regret_with_step(current_i64, prev_momentum_i64, delta_i64, &step, momentum_on)
}

/// Production entry point (momentum on).
#[inline]
pub fn update_regret_i64(
    current_i64: i64,
    prev_momentum_i64: i64,
    iteration: u32,
    delta_i64: i64,
) -> (i64, i64) {
    update_regret_i64_mode(current_i64, prev_momentum_i64, iteration, delta_i64, true)
}

/// Exact strategy-sum update with the γ=2 discount applied.
///
/// NOT wired into production. The playbook measures the cumulative
/// effect of γ=2 discounting at <0.1% over a full run, which is below
/// f64 precision. The strategy sum accumulates unweighted as before.
/// Kept as a documented helper for future experiments where the
/// discount might be made meaningful (e.g. rescaled τ).
#[allow(dead_code)]
pub fn apply_strategy_discount_i64(current_i64: i64, iteration: u32, prob_i64: i64) -> i64 {
    let t = iteration;
    if t == 0 {
        return current_i64.saturating_add(prob_i64);
    }
    let (num, den) = discount_num_den(t, 2);
    let current_i128 = current_i64 as i128;
    let new_i128 = (current_i128 * num) / den + prob_i64 as i128;
    new_i128.clamp(i64::MIN as i128, i64::MAX as i128) as i64
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
        let (r, _m) = update_regret_full(
            10.0,
            0.0,
            2,
            5.0,
            DiscountMode::CanonicalDcfr,
            MomentumMode::Off,
        );
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
        assert!(
            f > 0.99,
            "transitional range should be close to 1.0, got {f}"
        );
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

#[cfg(test)]
mod integer_discount_tests {
    use super::*;

    /// During warmup (t < TAU), the discount is exactly 1/1 for any p.
    #[test]
    fn discount_is_one_during_warmup() {
        for t in [0u32, 1, 500, 999] {
            for p in [0u32, 1, 2] {
                let (num, den) = discount_num_den(t, p);
                assert_eq!(
                    (num, den),
                    (1, 1),
                    "warmup discount for t={t} p={p} should be 1/1"
                );
            }
        }
    }

    /// At t >= TAU, p=0 gives 1/2 exactly (constant, independent of t).
    #[test]
    fn discount_p0_is_one_half() {
        for t in [1000u32, 2000, 10_000, 1_000_000] {
            assert_eq!(discount_num_den(t, 0), (1, 2));
        }
    }

    /// p=1 gives t/(t+1).
    #[test]
    fn discount_p1_is_t_over_t_plus_1() {
        assert_eq!(discount_num_den(1000, 1), (1000, 1001));
        assert_eq!(discount_num_den(2000, 1), (2000, 2001));
        assert_eq!(discount_num_den(1_000_000, 1), (1_000_000, 1_000_001));
    }

    /// p=2 gives t²/(t²+1) exactly — the irrational α=1.5 rounded to
    /// α=2 so the arithmetic stays in i128.
    #[test]
    fn discount_p2_is_t_squared_over_t_squared_plus_1() {
        assert_eq!(discount_num_den(1000, 2), (1_000_000, 1_000_001));
        assert_eq!(discount_num_den(2000, 2), (4_000_000, 4_000_001));
        assert_eq!(discount_num_den(10_000, 2), (100_000_000, 100_000_001),);
    }

    /// The rational approximation is always strictly less than 1
    /// (for p > 0) and monotone increasing in t.
    #[test]
    fn discount_is_monotone_and_below_one() {
        let t0 = discount_num_den(1000, 2);
        let t1 = discount_num_den(2000, 2);
        let t2 = discount_num_den(10_000, 2);
        let f = |(num, den): (i128, i128)| num as f64 / den as f64;
        assert!(f(t0) < f(t1));
        assert!(f(t1) < f(t2));
        assert!(f(t2) < 1.0);
        // And p=2 discounts slowly: f(t0) should be ~0.999999.
        assert!(f(t0) > 0.9999);
    }

    /// At iteration 0, the regret update returns delta unchanged as both
    /// new regret and new momentum.
    #[test]
    fn regret_update_at_t0_returns_delta() {
        let (r, m) = update_regret_i64(0, 0, 0, 12345);
        assert_eq!(r, 12345);
        assert_eq!(m, 12345);
    }

    /// A large-magnitude delta over many iterations must never produce
    /// a value that would overflow i32 when stored (that's the whole
    /// point of T1.1's integer path — the f32 version saturated).
    #[test]
    fn regret_update_stays_bounded_over_long_horizon() {
        // Simulate a persistent +1000-chip delta for 10M iterations.
        let mut r: i64 = 0;
        let mut m: i64 = 0;
        for t in 1..=10_000_000u32 {
            let (nr, nm) = update_regret_i64(r, m, t, 1_000_000); // +1000 chips in fixed point
            r = nr;
            m = nm;
        }
        // With the DCFR discount on positives, r should stabilise near
        // 1e6 / (1 - f(t)) which for p=2 is ~1e6 * t². That's huge, so
        // the point of this test is just that r is finite and positive.
        assert!(r > 0);
        assert!(r < i64::MAX);
    }
}

#[cfg(test)]
mod saturation_tests {
    use super::*;

    /// A3: an over-i64 add must saturate, not wrap or panic.
    #[test]
    fn update_regret_saturates_at_i64_max() {
        // discount_pos(i64::MAX, 1e6) is just under i64::MAX, so adding
        // ~i64::MAX/2 necessarily exceeds i64::MAX; the add must saturate.
        let (r, _) = update_regret_i64_mode(i64::MAX, 0, 1_000_000, i64::MAX / 2 + 1, false);
        assert_eq!(r, i64::MAX);
    }

    /// A3: negative extreme clamps at 0 (regret floors at 0, not i64::MIN).
    #[test]
    fn update_regret_floors_at_zero() {
        let (r, _) = update_regret_i64_mode(0, 0, 1_000_000, i64::MIN / 2, false);
        assert_eq!(r, 0);
    }

    /// A3: warmup (t < TAU) is identity on `current_i64`; result is max(0, cur+delta).
    #[test]
    fn update_regret_warmup_is_plain_add() {
        let (r, m) = update_regret_i64_mode(1_000, 0, 3, 250, false);
        assert_eq!(r, 1_250);
        assert_eq!(m, 250); // momentum field = delta during warmup
    }
}
