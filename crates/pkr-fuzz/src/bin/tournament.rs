//! `pkr-tournament` — head-to-head duplicate match between two
//! checkpoints.
//!
//! Usage:
//!   pkr-tournament --a <A.ckpt> --b <B.ckpt> \
//!     [--hands N] [--seed S] [--tables DIR]
//!
//! Both checkpoints must be loadable against the same abstraction
//! tables (which live in `--tables DIR`, or in A's parent directory).
//! A fingerprint mismatch aborts.
//!
//! Prints A's mean chip delta per deal over B, its standard error, and
//! the t-statistic. Positive mean means A beat B.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_fuzz::provider::TableProvider;
use pkr_fuzz::tournament::tournament;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn load_table(path: &Path, k: usize) -> CompactRegretTable {
    let t = CompactRegretTable::with_capacity(60_000_000);
    let fp = AbstractionFingerprint::from_constants(k as u32);
    t.load_checkpoint(path.to_str().unwrap(), &fp).unwrap_or_else(|e| {
        eprintln!("load checkpoint {}: {e}", path.display());
        std::process::exit(1);
    });
    t
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut a_path: Option<PathBuf> = None;
    let mut b_path: Option<PathBuf> = None;
    let mut tables: Option<PathBuf> = None;
    let mut hands: u32 = 20_000;
    let mut seed: u64 = 42;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--a" => { a_path = args.get(i + 1).map(PathBuf::from); i += 2; }
            "--b" => { b_path = args.get(i + 1).map(PathBuf::from); i += 2; }
            "--tables" => { tables = args.get(i + 1).map(PathBuf::from); i += 2; }
            "--hands" => { hands = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(hands); i += 2; }
            "--seed" => { seed = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(seed); i += 2; }
            _ => { i += 1; }
        }
    }

    let a_path = a_path.unwrap_or_else(|| {
        eprintln!("--a <checkpoint> is required");
        std::process::exit(2);
    });
    let b_path = b_path.unwrap_or_else(|| {
        eprintln!("--b <checkpoint> is required");
        std::process::exit(2);
    });

    // Abstraction tables live in --tables DIR, or in A's parent.
    let tdir = tables.unwrap_or_else(|| {
        a_path.parent().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
    });

    let store = load_centroids(tdir.join("centroids.bin").to_str().unwrap())
        .unwrap_or_else(|e| { eprintln!("load centroids: {e}"); std::process::exit(1); });
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, tdir.join("preflop_abstraction.bin").to_str().unwrap())
        .unwrap_or_else(|e| { eprintln!("init preflop: {e:?}"); std::process::exit(1); });
    abs.init_table(1, tdir.join("abstraction.bin").to_str().unwrap())
        .unwrap_or_else(|e| { eprintln!("init flop: {e:?}"); std::process::exit(1); });
    abs.init_table(2, tdir.join("turn_abstraction.bin").to_str().unwrap())
        .unwrap_or_else(|e| { eprintln!("init turn: {e:?}"); std::process::exit(1); });
    abs.init_table(3, tdir.join("river_buckets.bin").to_str().unwrap())
        .unwrap_or_else(|e| { eprintln!("init river: {e:?}"); std::process::exit(1); });

    let k = abs.k();
    eprintln!("abstraction: k={k} from {}", tdir.display());

    let table_a = load_table(&a_path, k);
    let table_b = load_table(&b_path, k);
    eprintln!("A: {} keys from {}", table_a.len(), a_path.display());
    eprintln!("B: {} keys from {}", table_b.len(), b_path.display());

    let pa = TableProvider::new(&table_a);
    let pb = TableProvider::new(&table_b);
    let ev = pkr_eval::NlheEvaluator;

    eprintln!("running {hands} deals, seed {seed} ...");
    let r = tournament(&pa, &pb, &abs, &ev, hands, seed);

    println!();
    println!("=== pkr-tournament ===");
    println!("  A: {}", a_path.display());
    println!("  B: {}", b_path.display());
    println!("  hands: {}  seed: {}", hands, seed);
    println!();
    println!("  A mean chips/deal (seat 0):  {:+.4}", r.mean_a_seat0);
    println!("  B mean chips/deal (seat 0):  {:+.4}", r.mean_b_seat0);
    println!("  diff (A - B):                {:+.4} chips/deal", r.mean_diff);
    println!("  SE:                          {:.4}", r.se_diff);
    println!("  t:                           {:+.2}", r.t);
    if r.t.abs() < 2.0 {
        println!();
        println!("  note: |t| < 2. This comparison is not statistically");
        println!("        significant at the 95% level. Run more hands.");
    }
}
