//! Run DCFR discount × momentum combinations on Kuhn poker to a fixed
//! iteration count and print exploitability at log-spaced checkpoints.
//!
//! The two questions:
//!   1. Does canonical discount (bounded) beat ratio-power (unbounded)?
//!   2. Does PCFR+ momentum accelerate convergence or block it?
//!
//! Run: cargo run --release -p pkr-testgames --bin kuhn-experiment

use pkr_cfr::dcfr::{DiscountMode, MomentumMode};
use pkr_testgames::kuhn::KuhnCfr;

const CHECKPOINTS: [u32; 10] = [
    100, 300, 1_000, 3_000, 10_000, 30_000, 100_000, 300_000, 1_000_000, 3_000_000,
];

struct Config {
    label: &'static str,
    discount: DiscountMode,
    momentum: MomentumMode,
}

fn main() {
    let configs = [
        Config { label: "vanilla", discount: DiscountMode::None, momentum: MomentumMode::Off },
        Config { label: "van-mom", discount: DiscountMode::None, momentum: MomentumMode::On },
        Config { label: "canon", discount: DiscountMode::CanonicalDcfr, momentum: MomentumMode::Off },
        Config { label: "canon-mom", discount: DiscountMode::CanonicalDcfr, momentum: MomentumMode::On },
        Config { label: "ratio", discount: DiscountMode::RatioPower, momentum: MomentumMode::Off },
        Config { label: "ratio-mom", discount: DiscountMode::RatioPower, momentum: MomentumMode::On },
    ];

    println!("=== Kuhn poker: discount × momentum ===");
    println!("Nash value to P0: -1/18 = -{:.6}", 1.0f32 / 18.0);
    println!();

    let mut runs: Vec<(&str, KuhnCfr)> = configs
        .iter()
        .map(|c| (c.label, KuhnCfr::new_full(c.discount, c.momentum)))
        .collect();

    // Header
    print!("{:>10}", "iter");
    for c in configs.iter() {
        print!("  {:>12}", c.label);
    }
    println!();

    let max_iter = CHECKPOINTS[CHECKPOINTS.len() - 1];
    let mut cp_idx = 0usize;

    for t in 1..=max_iter {
        for (_, cfr) in runs.iter_mut() {
            cfr.iterate();
        }
        if cp_idx < CHECKPOINTS.len() && t == CHECKPOINTS[cp_idx] {
            print!("{:>10}", t);
            for (_, cfr) in runs.iter() {
                if cfr.nan_flag {
                    print!("  {:>12}", "NaN");
                } else {
                    print!("  {:>12.3e}", cfr.exploitability());
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
            "  {:<12}  expl={:.3e}  value={:+.6}  max|reg|={:.3e}  NaN={}",
            name, e, v, max_r, cfr.nan_flag
        );
    }

    println!();
    let mut best: Option<(&str, f32)> = None;
    let mut disq: Vec<&str> = Vec::new();
    for (name, cfr) in runs.iter() {
        if cfr.nan_flag {
            disq.push(name);
            continue;
        }
        let e = cfr.exploitability();
        if e.is_finite() && best.map_or(true, |(_, b)| e < b) {
            best = Some((name, e));
        }
    }
    println!(
        "DISQUALIFIED (NaN in regrets): {}",
        if disq.is_empty() { "(none)".to_string() } else { disq.join(", ") }
    );
    match best {
        Some((name, val)) => println!(
            "VERDICT: best_mode={} best_exploitability={:.3e}",
            name, val
        ),
        None => println!("VERDICT: no finite mode — investigate."),
    }
}
