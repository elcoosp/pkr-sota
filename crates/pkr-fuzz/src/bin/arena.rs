//! `pkr-arena` — evaluate a trained checkpoint against the scripted
//! bot suite.
//!
//! Usage:
//!   pkr-arena \
//!     --checkpoint outputs/v41-post-f6/train.ckpt \
//!     --centroids  outputs/v41-post-f6/centroids.bin \
//!     --preflop-table outputs/v41-post-f6/preflop_abstraction.bin \
//!     --flop-table    outputs/v41-post-f6/abstraction.bin \
//!     --turn-table    outputs/v41-post-f6/turn_abstraction.bin \
//!     --river-table   outputs/v41-post-f6/river_buckets.bin \
//!     --hands 5000 --seed 42
//!
//! Reports bb/100 against each of the scripted bots and the aggregate
//! blueprint-hit rate. This is the F9 item-1 evaluator: an honest
//! measure of whether the checkpoint beats real (if simple) opponents.
//!
//! Before the audit there was no way to run this. `run_eval_harness`
//! existed, but nothing wired a real `CompactRegretTable` into it.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::{BlueprintProvider, SotaAdvice};
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_fuzz::{run_eval_harness, EvalContext};
use std::sync::Arc;

/// Wrap a `CompactRegretTable` as a `BlueprintProvider`.
///
/// `lookup` reads the average strategy, converts it to a u8 CDF, and
/// returns it in `SotaAdvice`. This is the same encoding the exporter
/// uses, so a checkpoint evaluated through this path and a blueprint
/// evaluated through `SolverHandle` should give the same bb/100.
struct TableProvider<'a> {
    table: &'a CompactRegretTable,
}

impl<'a> BlueprintProvider for TableProvider<'a> {
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        let strat = self.table.get_average_strategy_slice(infoset_hash)?;
        // Encode as a cumulative CDF over 6 buckets.
        let total: f32 = strat.iter().sum();
        let mut out = [0u8; 16];
        let mut cum = 0.0f32;
        let mut prev = 0u8;
        for i in 0..6 {
            cum += if total > 1e-9 { strat[i] / total } else { 0.0 };
            let b = (cum * 255.0).round().clamp(0.0, 255.0) as u8;
            out[i] = b.max(prev);
            prev = out[i];
        }
        // Extend to 16 slots with 255 (the exporter's convention).
        for i in 6..16 {
            out[i] = 255;
        }
        Some(SotaAdvice {
            cdf_probabilities: out,
            len: 6,
        })
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: {} --checkpoint <path> [--hands N] [--seed S]", args[0]);
        std::process::exit(2);
    }

    let mut checkpoint = None;
    let mut centroids = None;
    let mut preflop = None;
    let mut flop = None;
    let mut turn = None;
    let mut river = None;
    let mut hands: u32 = 5000;
    let mut seed: u64 = 42;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--checkpoint" => { checkpoint = args.get(i + 1).cloned(); i += 2; }
            "--centroids"  => { centroids  = args.get(i + 1).cloned(); i += 2; }
            "--preflop-table" => { preflop = args.get(i + 1).cloned(); i += 2; }
            "--flop-table"    => { flop    = args.get(i + 1).cloned(); i += 2; }
            "--turn-table"    => { turn    = args.get(i + 1).cloned(); i += 2; }
            "--river-table"   => { river   = args.get(i + 1).cloned(); i += 2; }
            "--hands" => { hands = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(hands); i += 2; }
            "--seed"  => { seed  = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(seed); i += 2; }
            _ => { i += 1; }
        }
    }

    let checkpoint = checkpoint.unwrap_or_else(|| {
        eprintln!("--checkpoint is required");
        std::process::exit(2);
    });

    // Abstraction tables live next to the checkpoint unless overridden.
    let dir = std::path::Path::new(&checkpoint)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let c = centroids.unwrap_or_else(|| dir.join("centroids.bin").to_string_lossy().into_owned());
    let pf = preflop.unwrap_or_else(|| dir.join("preflop_abstraction.bin").to_string_lossy().into_owned());
    let fl = flop.unwrap_or_else(|| dir.join("abstraction.bin").to_string_lossy().into_owned());
    let tn = turn.unwrap_or_else(|| dir.join("turn_abstraction.bin").to_string_lossy().into_owned());
    let rv = river.unwrap_or_else(|| dir.join("river_buckets.bin").to_string_lossy().into_owned());

    let store = load_centroids(&c).unwrap_or_else(|e| {
        eprintln!("load centroids: {e}");
        std::process::exit(1);
    });
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, &pf).unwrap_or_else(|e| { eprintln!("init preflop: {e:?}"); std::process::exit(1); });
    abs.init_table(1, &fl).unwrap_or_else(|e| { eprintln!("init flop: {e:?}"); std::process::exit(1); });
    abs.init_table(2, &tn).unwrap_or_else(|e| { eprintln!("init turn: {e:?}"); std::process::exit(1); });
    abs.init_table(3, &rv).unwrap_or_else(|e| { eprintln!("init river: {e:?}"); std::process::exit(1); });

    let k = abs.k();
    eprintln!("abstraction loaded: k={k}");

    let table = CompactRegretTable::with_capacity(60_000_000);
    let fp = AbstractionFingerprint::from_constants(k as u32);
    table.load_checkpoint(&checkpoint, &fp).unwrap_or_else(|e| {
        eprintln!("load checkpoint: {e}");
        std::process::exit(1);
    });
    eprintln!("checkpoint loaded: {} keys", table.len());

    let provider = TableProvider { table: &table };
    let ev = pkr_eval::NlheEvaluator;
    let ctx = EvalContext {
        provider: &provider,
        abstraction: &abs,
        evaluator: &ev,
    };

    let result = run_eval_harness(&ctx, hands, seed);

    println!();
    println!("=== arena: {} hands, seed {} ===", hands, seed);
    println!(
        "  bot bb/100: {:.2}  (decisions={} blueprint_hits={} fallback_hits={})",
        result.bot_bb_per_100, result.decisions, result.blueprint_hits, result.fallback_hits
    );
    for opp in &result.opponents {
        println!("    vs {:<10} {:+.2} bb/100", opp.name, opp.bb_per_100);
    }
    if result.decisions > 0 {
        let hit_pct = 100.0 * result.blueprint_hits as f64 / result.decisions as f64;
        println!("  blueprint hit rate: {:.1}%", hit_pct);
        if hit_pct < 5.0 {
            eprintln!();
            eprintln!("WARNING: blueprint hit rate below 5%. The bb/100 above");
            eprintln!("is dominated by the fallback path, not the trained strategy.");
            eprintln!("Check that the abstraction tables match the checkpoint's.");
        }
    }
}
