//! End-to-end eval-harness integration test.
//!
//! Loads a real trained blueprint plus its abstraction tables from env
//! vars, builds an EvalContext, and plays N hands against the three
//! scripted bots. Ignored by default because it requires artifacts on
//! disk. Run via:
//!
//!   PKR_BLUEPRINT=outputs/v0-smoke/blueprint.bin \
//!   PKR_CENTROIDS=outputs/v0-smoke/centroids.bin \
//!   PKR_PREFLOP_TABLE=outputs/v0-smoke/preflop_abstraction.bin \
//!   PKR_FLOP_TABLE=outputs/v0-smoke/flop_abstraction.bin \
//!   PKR_TURN_TABLE=outputs/v0-smoke/turn_abstraction.bin \
//!   PKR_RIVER_TABLE=outputs/v0-smoke/river_buckets.bin \
//!   PKR_FLOP_BUCKETS=outputs/v0-smoke/flop_buckets.bin \
//!   PKR_RANK_TABLE=outputs/v0-smoke/hand_ranks.bin \
//!   cargo test --release -p pkr-trainer --test eval_harness -- --ignored --nocapture
//!
//! Exits cleanly if artifacts are missing (like load_external_blueprint).

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_eval::TableEvaluator;
use pkr_fuzz::{run_eval_harness, EvalContext};
use pkr_runtime::{MmapReader, SolverHandle};
use std::sync::Arc;

#[test]
#[ignore]
fn eval_harness_runs_against_real_blueprint() {
    // Resolve any relative path against the workspace root, not the
    // package directory. cargo test sets CWD to binaries/pkr-trainer/.
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("expected crates/pkr-trainer to have two parents")
        .to_path_buf();
    let abs = |p: String| -> std::path::PathBuf {
        let pb = std::path::PathBuf::from(&p);
        if pb.is_absolute() { pb } else { workspace_root.join(pb) }
    };

    let bp = match std::env::var("PKR_BLUEPRINT") {
        Ok(p) => abs(p),
        Err(_) => {
            eprintln!("PKR_BLUEPRINT not set; skipping");
            return;
        }
    };
    let centroids_path = abs(std::env::var("PKR_CENTROIDS")
        .expect("PKR_CENTROIDS must be set alongside PKR_BLUEPRINT"));
    let rank_table_path = abs(std::env::var("PKR_RANK_TABLE")
        .expect("PKR_RANK_TABLE must be set"));

    // Build evaluator (needed by abstraction EHS fallback + harness).
    let evaluator = Arc::new(
        TableEvaluator::new(&rank_table_path).expect("failed to load rank table"),
    );

    // Build abstraction with whatever tables are available.
    let store = load_centroids(centroids_path.to_str().unwrap()).expect("failed to load centroids");
    let abstraction = KMeansAbstraction::from_store(store, evaluator.clone());
    if let Ok(p) = std::env::var("PKR_PREFLOP_TABLE") {
        abstraction.init_table(0, abs(p).to_str().unwrap()).ok();
    }
    if let Ok(p) = std::env::var("PKR_FLOP_TABLE") {
        abstraction.init_table(1, abs(p).to_str().unwrap()).ok();
    }
    if let Ok(p) = std::env::var("PKR_TURN_TABLE") {
        abstraction.init_table(2, abs(p).to_str().unwrap()).ok();
    }
    if let Ok(p) = std::env::var("PKR_RIVER_TABLE") {
        abstraction.init_table(3, abs(p).to_str().unwrap()).ok();
    }
    if let Ok(p) = std::env::var("PKR_FLOP_BUCKETS") {
        abstraction.load_flop_buckets(abs(p).to_str().unwrap()).ok();
    }

    // Load blueprint through the runtime path.
    let reader = MmapReader::new(&bp).expect("failed to open blueprint");
    let solver = SolverHandle::new(reader);

    let ctx = EvalContext {
        provider: &solver,
        abstraction: &abstraction,
        evaluator: evaluator.as_ref(),
    };

    let num_hands = std::env::var("PKR_EVAL_HANDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200u32);

    eprintln!("=== eval harness: {} hands vs each scripted bot ===", num_hands);
    let result = run_eval_harness(&ctx, num_hands, 0xDEAD_BEEF_CAFE_1234u64);

    for opp in &result.opponents {
        eprintln!(
            "  vs {:<12}  bb/100 = {:+8.2}",
            opp.name, opp.bb_per_100
        );
    }
    eprintln!("  mean bb/100 = {:+.2}", result.bot_bb_per_100);

    // Sanity assertions. We are not asserting profitability — that would
    // require the blueprint to actually be trained. We assert only that
    // the harness produced finite numbers over the requested hands.
    assert_eq!(result.opponents.len(), 3, "expected 3 opponents");
    for opp in &result.opponents {
        assert!(
            opp.bb_per_100.is_finite(),
            "opponent {} produced non-finite bb/100",
            opp.name
        );
        assert!(
            opp.bb_per_100.abs() < 1000.0,
            "opponent {} bb/100 {} out of plausible range",
            opp.name,
            opp.bb_per_100
        );
    }
}
