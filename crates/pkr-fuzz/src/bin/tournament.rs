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
    let mut search_a = false;
    let mut search_b = false;
    let mut search_iters: u32 = 50;
    let mut search_hands: usize = 12;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--a" => { a_path = args.get(i + 1).map(PathBuf::from); i += 2; }
            "--b" => { b_path = args.get(i + 1).map(PathBuf::from); i += 2; }
            "--tables" => { tables = args.get(i + 1).map(PathBuf::from); i += 2; }
            "--hands" => { hands = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(hands); i += 2; }
            "--seed" => { seed = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(seed); i += 2; }
            "--search-a" => { search_a = true; i += 1; }
            "--search-b" => { search_b = true; i += 1; }
            "--search-iters" => { search_iters = args.get(i+1).and_then(|s| s.parse().ok()).unwrap_or(search_iters); i += 2; }
            "--search-hands" => { search_hands = args.get(i+1).and_then(|s| s.parse().ok()).unwrap_or(search_hands); i += 2; }
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
    let td = pkr_fuzz::tournament::resolve_tables_dir(
        &a_path,
        &b_path,
        tables.as_deref(),
    );
    if td.a_b_diverge {
        eprintln!(
            "ERROR: A and B live in different directories and no --tables DIR was given. \
             Both would load against A's tables ({}), silently comparing two different \
             abstractions. Pass --tables DIR explicitly.",
            td.dir.display(),
        );
        std::process::exit(1);
    }
    let tdir = td.dir;

    // Task 0.2: a quoted H2H number needs >= 2000 duplicate deals.
    // Below that the paired SE is too wide to resolve typical effects.
    if hands < 2000 {
        eprintln!(
            "WARNING: --hands {} is below the 2000-deal floor for a quoted \
             H2H reading (paired SE will be wide).",
            hands
        );
    }

    let store = load_centroids(tdir.join("centroids.bin").to_str().unwrap())
        .unwrap_or_else(|e| { eprintln!("load centroids: {e}"); std::process::exit(1); });
    let abs = Arc::new(KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator)));
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

    let table_a = Arc::new(load_table(&a_path, k));
    let table_b = Arc::new(load_table(&b_path, k));
    eprintln!("A: {} keys from {}", table_a.len(), a_path.display());
    eprintln!("B: {} keys from {}", table_b.len(), b_path.display());

    let pa = TableProvider::new(&table_a);
    let pb = TableProvider::new(&table_b);
    let ev = pkr_eval::NlheEvaluator;

    // Optional per-side subgame search. Same checkpoint, one side searched,
    // isolates search's head-to-head effect.
    let mk_handle = |search: bool, table: &Arc<pkr_cfr::table::CompactRegretTable>|
        -> Option<Box<dyn pkr_runtime::subgame::SearchHandle>> {
        if !search { return None; }
        let cfg = pkr_runtime::subgame::SubgameConfig {
            evaluator: Arc::new(pkr_eval::NlheEvaluator),
            abstraction: abs.clone() as Arc<dyn pkr_contracts::AbstractionBuilder>,
            table: table.clone(),
            iters: search_iters,
            hands_per_range: search_hands,
            enabled_streets: [false, false, true, true],
        };
        Some(Box::new(pkr_runtime::subgame::SubgameHandle::new(cfg)))
    };
    let handle_a = mk_handle(search_a, &table_a);
    let handle_b = mk_handle(search_b, &table_b);
    if search_a || search_b {
        eprintln!("search: A={search_a} B={search_b} ({search_iters} iters, {search_hands} hands/range)");
    }

    eprintln!("running {hands} deals, seed {seed} ...");
    let r = tournament(&pa, &pb, abs.as_ref(), &ev, hands, seed, handle_a.as_deref(), handle_b.as_deref());

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
    // A vs A gives mean_diff = 0 and se_diff = 0; that's the correct
    // degenerate answer, not a "run more hands" situation.
    if r.mean_diff == 0.0 && r.se_diff == 0.0 {
        println!();
        println!("  note: identical inputs, so the diff is exactly zero.");
    } else if r.t.abs() < 2.0 {
        println!();
        println!("  note: |t| < 2. This comparison is not statistically");
        println!("        significant at the 95% level. Run more hands.");
    }
}
