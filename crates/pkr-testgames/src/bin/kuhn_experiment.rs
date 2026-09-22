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
        let max_r = cfr.max_abs_regret();
        println!(
            "  {:<14}  exploitability = {:.3e}   value = {:+.6}   max|regret| = {:.3e}   NaN = {}",
            name, e, v, max_r, cfr.nan_flag
        );
    }

    // Verdict: lowest finite exploitability at the final checkpoint wins.
    // A mode that produced NaN is disqualified, not "best".
    let mut best: Option<(&str, f32)> = None;
    let mut disqualified: Vec<&str> = Vec::new();
    for (name, cfr) in runs.iter() {
        if cfr.nan_flag {
            disqualified.push(name);
            continue;
        }
        let e = cfr.exploitability();
        if e.is_finite() && best.map_or(true, |(_, b)| e < b) {
            best = Some((name, e));
        }
    }
    println!();
    println!(
        "DISQUALIFIED (NaN in regrets): {}",
        if disqualified.is_empty() {
            "(none)".to_string()
        } else {
            disqualified.join(", ")
        }
    );
    match best {
        Some((name, val)) => println!(
            "VERDICT: best_mode={} best_exploitability={:.3e}",
            name, val
        ),
        None => println!("VERDICT: no finite mode — investigate immediately."),
    }
}
