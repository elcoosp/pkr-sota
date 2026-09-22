//! Run three DCFR discount modes on Kuhn poker to a fixed iteration count
//! and print exploitability at log-spaced checkpoints. The winning mode
//! is whichever reaches the lowest exploitability the fastest.
//!
//! Run: cargo run --release -p pkr-testgames --bin kuhn-experiment

use pkr_cfr::dcfr::DiscountMode;
use pkr_testgames::kuhn::KuhnCfr;

const CHECKPOINTS: [u32; 10] = [
    100, 300, 1_000, 3_000, 10_000, 30_000, 100_000, 300_000, 1_000_000, 3_000_000,
];

fn main() {
    let modes = [
        ("none", DiscountMode::None),
        ("canonical", DiscountMode::CanonicalDcfr),
        ("ratio-power", DiscountMode::RatioPower),
    ];

    // Nash value to P0 in Kuhn: -1/18. Exploitability of exact Nash: 0.
    println!("=== Kuhn poker DCFR comparison ===");
    println!("Nash value to P0: -1/18 = -{:.6}", 1.0f32 / 18.0);
    println!();
    println!(
        "{:>10}  {:>14}  {:>14}  {:>14}",
        "iter", "none", "canonical", "ratio-power"
    );

    let mut runs: Vec<(&str, KuhnCfr)> = modes
        .iter()
        .map(|(name, m)| (*name, KuhnCfr::new(*m)))
        .collect();

    let max_iter = CHECKPOINTS[CHECKPOINTS.len() - 1];
    let mut cp_idx = 0usize;

    for t in 1..=max_iter {
        for (_, cfr) in runs.iter_mut() {
            cfr.iterate();
        }
        if cp_idx < CHECKPOINTS.len() && t == CHECKPOINTS[cp_idx] {
            print!("{:>10}  ", t);
            for (_, cfr) in runs.iter() {
                let e = cfr.exploitability();
                if cfr.nan_flag {
                    print!("{:>14}  ", "NaN");
                } else {
                    print!("{:>14.3e}  ", e);
                }
            }
            println!();
            cp_idx += 1;
        }
    }

    println!();
    println!("=== Final values at t={} ===", max_iter);
    for (name, cfr) in runs.iter() {
        let e = cfr.exploitability();
        let v = cfr.value_of_avg();
        println!(
            "  {:<14}  exploitability = {:.3e}   value = {:+.6}   NaN = {}",
            name, e, v, cfr.nan_flag
        );
    }

    // Verdict: lowest exploitability at the final checkpoint wins.
    let mut best = (runs[0].0, runs[0].1.exploitability());
    for (name, cfr) in runs.iter().skip(1) {
        let e = cfr.exploitability();
        if e < best.1 {
            best = (name, e);
        }
    }
    let current = runs
        .iter()
        .find(|(n, _)| *n == "ratio-power")
        .map(|(_, c)| c.exploitability())
        .unwrap_or(f32::INFINITY);
    let ratio = current / best.1.max(1e-12);
    println!();
    println!(
        "VERDICT: best_mode={} best_exploitability={:.3e} ratio_power_vs_best={:.2}x",
        best.0, best.1, ratio
    );
}
