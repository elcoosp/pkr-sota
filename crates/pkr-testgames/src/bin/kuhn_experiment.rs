//! Run DCFR discount × momentum combinations on Kuhn poker to a fixed
//! iteration count and print exploitability at log-spaced checkpoints,
//! plus a strategy dump at the end so non-convergence is debuggable.
//!
//! Run: cargo run --release -p pkr-testgames --bin kuhn-experiment

#![allow(clippy::needless_range_loop)]  // numerics: indexed loops are idiomatic here

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
        Config { label: "vanilla",   discount: DiscountMode::None,          momentum: MomentumMode::Off },
        Config { label: "van-mom",   discount: DiscountMode::None,          momentum: MomentumMode::On  },
        Config { label: "canon",     discount: DiscountMode::CanonicalDcfr, momentum: MomentumMode::Off },
        Config { label: "canon-mom", discount: DiscountMode::CanonicalDcfr, momentum: MomentumMode::On  },
    ];

    println!("=== Kuhn poker: discount x momentum ===");
    println!("Nash value to P0: -1/18 = -{:.6}", 1.0f32 / 18.0);
    println!();

    let mut runs: Vec<(&str, KuhnCfr)> = configs
        .iter()
        .map(|c| (c.label, KuhnCfr::new_full(c.discount, c.momentum)))
        .collect();

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
    println!("=== Strategy dumps (avg strategy at t={}) ===", max_iter);
    println!();
    println!("Nash reference:");
    println!("  P0 J dp=0:  check=2/3  bet=1/3");
    println!("  P0 Q dp=0:  check=1.0  bet=0.0");
    println!("  P0 K dp=0:  check=0.0  bet=1.0  (or 1/3 bet, but pure bet typical)");
    println!("  P0 Q dp=1:  fold=2/3   call=1/3");
    println!("  P1 J dp=1:  fold=1.0   call=0.0");
    println!("  P1 Q dp=1:  fold=1/3   call=2/3");
    println!("  P1 K dp=1:  fold=0.0   call=1.0");
    println!();

    for (name, cfr) in runs.iter() {
        if cfr.nan_flag {
            println!("  [{}] (skipped: NaN)", name);
            continue;
        }
        println!("  [{}]", name);
        for i in 0..12usize {
            let s = cfr.average_strategy_at(i);
            let player = i / 6;
            let card_idx = (i % 6) / 2;
            let dp = i % 2;
            let card_name = ["J", "Q", "K"][card_idx];
            let action_names: [&str; 2] = match (player, dp) {
                (0, 0) => ["check", "bet"],
                (0, 1) => ["fold",  "call"],
                (1, 0) => ["check", "bet"],
                (1, 1) => ["fold",  "call"],
                _ => ["a0", "a1"],
            };
            println!(
                "    P{} {} dp={}:  {:>5}={:.4}  {:>5}={:.4}",
                player, card_name, dp,
                action_names[0], s[0],
                action_names[1], s[1],
            );
        }
        println!();
    }

    println!("=== Verdict ===");
    let mut best: Option<(&str, f32)> = None;
    let mut disq: Vec<&str> = Vec::new();
    for (name, cfr) in runs.iter() {
        if cfr.nan_flag {
            disq.push(name);
            continue;
        }
        let e = cfr.exploitability();
        if e.is_finite() && best.is_none_or(|(_, b)| e < b) {
            best = Some((name, e));
        }
    }
    println!(
        "DISQUALIFIED (NaN in regrets): {}",
        if disq.is_empty() { "(none)".to_string() } else { disq.join(", ") }
    );
    match best {
        Some((name, val)) => println!(
            "best_mode={} best_exploitability={:.3e}",
            name, val
        ),
        None => println!("no finite mode."),
    }
}
