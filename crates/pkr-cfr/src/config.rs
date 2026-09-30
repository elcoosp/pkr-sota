//! Single source of truth for training-affecting hyperparameters.
//!
//! Before this module, hyperparameters were read from the environment
//! at every call site via `std::env::var(...)`. That produced three
//! problems the audit called out (F2):
//!
//! 1. **Code defaults didn't match what the experiment scripts set.**
//!    Momentum defaulted ON in code (docs call it structurally wrong),
//!    `PKR_AVG_POWER` defaulted to 0 (uniform averaging), and
//!    `PKR_EXPLORE_EPSILON` defaulted to 0.05 while every A/B script
//!    used 0.01. Runs launched with `./run.sh` therefore did not
//!    reproduce the published numbers.
//! 2. **`stats.json` misdescribed what ran.** It reported `momentum=on`
//!    and `avg_power=2` (the historical defaults) regardless of the
//!    effective values.
//! 3. **No way to see the whole config at a glance.**
//!
//! This struct is read once, cached, and used everywhere. The trainer
//! serializes it into `stats.json` so a checkpoint's config is always
//! recoverable.
//!
//! # Defaults
//!
//! The defaults are the **experiment configuration** that produced the
//! v33/v34/v38 findings — not the historical code defaults. Any run
//! that does not explicitly export a variable gets the same behavior
//! the A/B scripts used. That makes `./run.sh` with a clean environment
//! reproducible.
//!
//! Set `PKR_MOMENTUM=1` etc. to opt in to the old behavior.

use std::sync::OnceLock;

/// Env-var fallback helper: read `name`, parse, keep only if the
/// predicate passes. Falls back to `default` on any error.
fn env_parse<T: std::str::FromStr>(
    name: &str,
    default: T,
    accept: impl Fn(&T) -> bool,
) -> T {
    std::env::var(name)
        .ok()
        .and_then(|s| s.parse::<T>().ok())
        .filter(|v| accept(v))
        .unwrap_or(default)
}

/// `PKR_MOMENTUM`: read as a boolean. Unset or any non-truthy value
/// means OFF (the experiment config). Truthy values are `1`, `on`,
/// `true`, `yes`.
fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name).as_deref() {
        Ok("1") | Ok("on") | Ok("true") | Ok("yes") => true,
        Ok("0") | Ok("off") | Ok("false") | Ok("no") => false,
        _ => default,
    }
}

/// Training-affecting configuration, resolved once per process.
#[derive(Debug, Clone, Copy)]
pub struct TrainConfig {
    /// PCFR+ momentum term in the regret update. The docs recommend
    /// OFF (the update is structurally wrong); the historical code
    /// default was ON. Now OFF by default.
    pub momentum: bool,
    /// Strategy-sum weight exponent. `0` = uniform, `1` = linear, `2` =
    /// DCFR gamma. Experiments used 2.
    pub avg_power: f32,
    /// ε-uniform exploration at opponent nodes. Experiments used 0.01.
    pub explore_epsilon: f32,
    /// DCFR α (positive-regret exponent). 1.5 is the canonical DCFR
    /// setting.
    pub dcfr_alpha: f64,
    /// Whether the HS-DCFR anneal (α 2.0→1.5, γ 2.0→1.0) is enabled.
    pub hs_dcfr: bool,
    /// Total iteration count for the HS-DCFR anneal schedule.
    pub hs_dcfr_total: f64,
    /// Alternating player updates (CFR+ semantics).
    pub alt_updates: bool,
    /// Phase profiling (dev-only; off in production).
    pub phase_profile: bool,
    /// RM+ flooring of sampled regrets. `false` = allow negative regrets
    /// to persist (audit F5 recommendation).
    pub neg_floor: bool,
    /// Sequential application of flushed batches. Production uses true;
    /// the audit F5 grid also tests false.
    pub sequential: bool,
    /// Strict-bet validation in `apply_action` (dev-only).
    pub strict_bets: bool,
    /// Skip-forced-nodes optimization (dev-only). Production ignores it.
    pub skip_forced: bool,
    /// F5: which nodes accumulate the average strategy.
    ///
    /// `true` (default) = the traverser's own nodes, weighted by
    /// `strategy · own_reach · t^p` (the historical scheme).
    ///
    /// `false` = the opponent's nodes, weighted by `strategy · t^p`
    /// only. That's the standard external-sampling averaging site: at
    /// opponent nodes the visit frequency already encodes the
    /// opponent's reach, so no reach factor is needed.
    ///
    /// Gated because it changes the convergence path; test on Kuhn or
    /// Leduc before enabling for NLHE. See docs/experiments/f5-grid.md.
    pub avg_at_traverser: bool,
}

impl Default for TrainConfig {
    fn default() -> Self {
        TrainConfig {
            momentum: false,
            avg_power: 2.0,
            explore_epsilon: 0.01,
            dcfr_alpha: 1.5,
            hs_dcfr: false,
            hs_dcfr_total: 200_000_000.0,
            alt_updates: false,
            phase_profile: false,
            neg_floor: true,
            sequential: true,
            strict_bets: false,
            skip_forced: false,
            avg_at_traverser: true,
        }
    }
}

impl TrainConfig {
    /// Read the process-wide config from the environment, or return the
    /// cached value from a previous call. The first call wins; later
    /// env mutations have no effect.
    pub fn global() -> &'static Self {
        static C: OnceLock<TrainConfig> = OnceLock::new();
        C.get_or_init(Self::from_env)
    }

    /// Read directly from the environment. Called once per process by
    /// `global()`; exposed for tests.
    pub fn from_env() -> Self {
        let d = TrainConfig::default();
        TrainConfig {
            momentum: env_bool("PKR_MOMENTUM", d.momentum),
            avg_power: env_parse("PKR_AVG_POWER", d.avg_power, |p| {
                (0.0..=4.0).contains(p)
            }),
            explore_epsilon: env_parse("PKR_EXPLORE_EPSILON", d.explore_epsilon, |e| {
                (0.0..1.0).contains(e)
            }),
            dcfr_alpha: env_parse("PKR_DCFR_ALPHA", d.dcfr_alpha, |v| (0.5..=3.0).contains(v)),
            hs_dcfr: env_bool("PKR_HS_DCFR", d.hs_dcfr),
            hs_dcfr_total: env_parse("PKR_HS_DCFR_TOTAL", d.hs_dcfr_total, |v| *v > 0.0),
            alt_updates: env_bool("PKR_ALT_UPDATES", d.alt_updates),
            phase_profile: env_bool("PKR_PHASE_PROFILE", d.phase_profile),
            neg_floor: env_bool("PKR_RM_PLUS", d.neg_floor),
            sequential: env_bool("PKR_F5_SEQUENTIAL", d.sequential),
            strict_bets: env_bool("PKR_STRICT_BETS", d.strict_bets),
            skip_forced: env_bool("PKR_SKIP_FORCED", d.skip_forced),
            avg_at_traverser: env_bool("PKR_AVG_AT_TRAVERSER", d.avg_at_traverser),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_experiment_config() {
        let c = TrainConfig::default();
        assert!(!c.momentum, "momentum must be OFF by default (F2)");
        assert_eq!(c.avg_power, 2.0, "experiments used avg_power=2");
        assert_eq!(c.explore_epsilon, 0.01, "experiments used eps=0.01");
        assert_eq!(c.dcfr_alpha, 1.5, "canonical DCFR alpha");
    }

    #[test]
    fn env_overrides_change_the_value() {
        // SAFETY: tests in this module run on the same thread by default
        // (nextest uses a fresh process per test). `std::env::set_var` is
        // unsafe in Rust 2024 edition; for 2021 it's still allowed.
        std::env::set_var("PKR_EXPLORE_EPSILON", "0.05");
        let c = TrainConfig::from_env();
        assert!((c.explore_epsilon - 0.05).abs() < 1e-9);
        std::env::remove_var("PKR_EXPLORE_EPSILON");
    }

    #[test]
    fn out_of_range_values_are_ignored() {
        std::env::set_var("PKR_EXPLORE_EPSILON", "5.0");
        let c = TrainConfig::from_env();
        // 5.0 fails the [0, 1) predicate, so default is used.
        assert!((c.explore_epsilon - 0.01).abs() < 1e-9);
        std::env::remove_var("PKR_EXPLORE_EPSILON");
    }
}
