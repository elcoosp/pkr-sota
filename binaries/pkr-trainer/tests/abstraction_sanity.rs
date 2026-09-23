//! Production abstraction sanity checks.
//!
//! These tests verify that the *actual* tables in `$PKR_ABS_DIR` (the
//! ones that will be used for the next training run) satisfy the
//! invariants every downstream consumer relies on. They are
//! `#[ignore]` by default because they need real tables on disk.
//!
//! Run via:
//!
//! ```text
//! PKR_ABS_DIR=outputs/v15 \
//!   cargo test --release -p pkr-trainer \
//!     --test abstraction_sanity -- --ignored --nocapture
//! ```
//!
//! What these tests would have caught (r3 §17):
//!
//!   - k=8 preflop abstraction: AA and 33 shared bucket 4; QQ and 72o
//!     shared bucket 1. Both are caught by `premium_vs_trash_separated`.
//!   - Preflop table with the wrong dimensions (e.g. a truncated file
//!     or a k=200 file mistakenly used as a k=8 file).
//!
//! The tests never touch training; they only read tables and hash
//! infosets. Safe to run alongside a live trainer.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_contracts::AbstractionBuilder;
use pkr_eval::TableEvaluator;
use std::sync::Arc;

/// Resolve a possibly-relative path against the workspace root.
fn resolve(p: &str) -> std::path::PathBuf {
    let pb = std::path::PathBuf::from(p);
    if pb.is_absolute() {
        return pb;
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("pkr-trainer has two parents");
    root.join(pb)
}

/// Load the abstraction configured by `PKR_ABS_DIR`. Returns None (and
/// prints a skip message) if the env var is not set.
fn load_abstraction() -> Option<KMeansAbstraction> {
    let dir = match std::env::var("PKR_ABS_DIR") {
        Ok(d) => d,
        Err(_) => {
            eprintln!("PKR_ABS_DIR not set; skipping");
            return None;
        }
    };
    let dir = resolve(&dir);
    let rank_table = dir.join("hand_ranks.bin");
    let centroids = dir.join("centroids.bin");
    if !rank_table.exists() || !centroids.exists() {
        eprintln!(
            "PKR_ABS_DIR={} missing hand_ranks.bin or centroids.bin; skipping",
            dir.display()
        );
        return None;
    }

    let evaluator = Arc::new(TableEvaluator::new(&rank_table).expect("load hand_ranks.bin"));
    let store = load_centroids(centroids.to_str().unwrap()).expect("load centroids");
    let a = KMeansAbstraction::from_store(store, evaluator);

    // Load every table that exists. Missing ones are simply not loaded;
    // the tests that don't touch them still run.
    for (code, name) in [
        (0u8, "preflop_abstraction.bin"),
        (1, "abstraction.bin"),
        (2, "turn_abstraction.bin"),
        (3, "river_buckets.bin"),
    ] {
        let p = dir.join(name);
        if p.exists() {
            a.init_table(code, p.to_str().unwrap()).expect("init_table");
        }
    }
    Some(a)
}

/// The k=8 pathology from r3 §17, encoded forever.
///
/// AA, 33, and 72o must map to *different* infosets under the current
/// abstraction. If they collide, the abstraction is too coarse and the
/// CFR will solve a compromise strategy for hands of wildly different
/// value.
#[test]
#[ignore]
fn premium_vs_trash_separated() {
    let abstraction = match load_abstraction() {
        Some(a) => a,
        None => return,
    };

    // Card encoding: rank = c >> 2, suit = c & 3.
    //   A♠=48, A♥=49, 3♠=4, 3♥=5, 7♠=20, 2♣=2
    let aa: [u8; 2] = [48, 49];
    let tt33: [u8; 2] = [4, 5];
    let trash: [u8; 2] = [20, 2];
    let qq: [u8; 2] = [40, 41];

    let history: [u8; 4] = [0, 0, 0, 0];
    let h_aa = abstraction.get_infoset_hash(&aa, &[], &history, 0);
    let h_33 = abstraction.get_infoset_hash(&tt33, &[], &history, 0);
    let h_72o = abstraction.get_infoset_hash(&trash, &[], &history, 0);
    let h_qq = abstraction.get_infoset_hash(&qq, &[], &history, 0);

    assert_ne!(
        h_aa, h_33,
        "AA and 33 share an infoset — this is the k=8 pathology (r3 §17)"
    );
    assert_ne!(
        h_aa, h_72o,
        "AA and 72o share an infoset — this is the k=8 pathology (r3 §17)"
    );
    assert_ne!(
        h_qq, h_72o,
        "QQ and 72o share an infoset — this is the k=8 pathology (r3 §17)"
    );
}

/// Centroid count floor. Catches a k=8 table being used where k≥100 is
/// expected, without needing to count bytes on disk.
#[test]
#[ignore]
fn centroids_k_at_least_100() {
    let dir = match std::env::var("PKR_ABS_DIR") {
        Ok(d) => resolve(&d),
        Err(_) => {
            eprintln!("PKR_ABS_DIR not set; skipping");
            return;
        }
    };
    let centroids = dir.join("centroids.bin");
    if !centroids.exists() {
        eprintln!("{} missing; skipping", centroids.display());
        return;
    }
    let store = load_centroids(centroids.to_str().unwrap()).expect("load centroids");
    let k = store.centroids.len();
    assert!(
        k >= 100,
        "centroids.bin has k={} — below the production floor of 100. \
         This is the smoke-test configuration that caused the v9–v13 \
         incident (r3 §17).",
        k
    );
    eprintln!("centroids: k={}", k);
}

/// Preflop table has exactly 1326 entries (C(52,2)).
#[test]
#[ignore]
fn preflop_table_has_1326_entries() {
    let dir = match std::env::var("PKR_ABS_DIR") {
        Ok(d) => resolve(&d),
        Err(_) => {
            eprintln!("PKR_ABS_DIR not set; skipping");
            return;
        }
    };
    let p = dir.join("preflop_abstraction.bin");
    if !p.exists() {
        eprintln!("{} missing; skipping", p.display());
        return;
    }
    let bytes = std::fs::read(&p).expect("read preflop table");
    assert_eq!(
        bytes.len(),
        1326,
        "preflop_abstraction.bin is {} bytes, expected 1326",
        bytes.len()
    );
}

/// Hash stability across threads using the real abstraction (not a
/// synthetic store). This is the load-bearing property for parallel
/// training: the traverser computes hashes from many rayon workers.
#[test]
#[ignore]
fn real_abstraction_hash_is_thread_stable() {
    use rayon::prelude::*;
    let abstraction = match load_abstraction() {
        Some(a) => a,
        None => return,
    };
    // A representative non-trivial state: AA preflop, empty history.
    let hole: [u8; 2] = [48, 49];
    let history: [u8; 4] = [0, 0, 0, 0];
    let expected = abstraction.get_infoset_hash(&hole, &[], &history, 0);
    let hashes: Vec<u64> = (0..4096)
        .into_par_iter()
        .map(|_| abstraction.get_infoset_hash(&hole, &[], &history, 0))
        .collect();
    for h in hashes {
        assert_eq!(h, expected);
    }
}
