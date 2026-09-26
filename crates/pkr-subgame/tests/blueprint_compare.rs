//! POC: river CFR vs trained blueprint.
//!
//! Loads v34long's abstraction and checkpoint, then compares three P0
//! strategies on the same river subgame:
//!   - uniform (no learning baseline)
//!   - CFR-solved (fresh 200-iter solve)
//!   - blueprint (v34long 100M-iter trained strategy)
//!
//! The number that matters: if `BR_v1 vs CFR < BR_v1 vs blueprint`, a
//! fresh subgame solve beats the trained blueprint at this specific river
//! state, which justifies building the full search stack.
//!
//! Run with:
//!   cargo nextest run -p pkr-subgame --run-ignored all --nocapture poc_river_vs_blueprint

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_subgame::{run_poc, POCConfig, Range};
use std::sync::Arc;

/// Absolute path to the workspace root. Nextest runs tests with CWD set
/// to the crate root, so all `outputs/` lookups need to be prefixed.
fn workspace_root() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR = .../crates/pkr-subgame
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().to_path_buf()
}

fn out(rel: &str) -> String {
    workspace_root().join("outputs/v34long").join(rel)
        .to_string_lossy().into_owned()
}

fn board() -> [u8; 5] {
    [0, 14, 28, 42, 8]
}

fn make_range(pool: &[u8], exclude: &[u8], n: usize) -> Vec<[u8; 2]> {
    let available: Vec<u8> = pool.iter().copied().filter(|c| !exclude.contains(c)).collect();
    let mut hands = Vec::new();
    'outer: for i in 0..available.len() {
        for j in (i + 1)..available.len() {
            hands.push([available[i], available[j]]);
            if hands.len() >= n { break 'outer; }
        }
    }
    hands
}

fn river_root(b: &[u8; 5]) -> GameState {
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.advance_street_in_place(&b[0..3]);
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.advance_street_in_place(&b[3..4]);
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.advance_street_in_place(&b[4..5]);
    s
}

#[test]
#[ignore]
fn poc_river_vs_blueprint() {
    let b = board();
    let p0_pool: Vec<u8> = (0u8..26).collect();
    let p1_pool: Vec<u8> = (26u8..52).collect();
    let p0_hands = make_range(&p0_pool, &b, 10);
    let p1_hands = make_range(&p1_pool, &b, 10);
    println!("board    = {:?}", b);
    println!("P0 range = {} hands", p0_hands.len());
    println!("P1 range = {} hands", p1_hands.len());

    // ---- Build the abstraction ----
    let centroids_store = load_centroids(&out("centroids.bin"))
        .expect("load centroids");
    let abs = KMeansAbstraction::from_store(centroids_store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, &out("preflop_abstraction.bin")).expect("preflop table");
    abs.init_table(1, &out("abstraction.bin")).expect("flop table");
    abs.init_table(2, &out("turn_abstraction.bin")).expect("turn table");
    abs.init_table(3, &out("river_buckets.bin")).expect("river table");
    let abs_arc: Arc<dyn AbstractionBuilder> = Arc::new(abs);

    // ---- Load the checkpoint ----
    let evaluator = Arc::new(pkr_eval::NlheEvaluator);
    let trainer = Trainer::with_capacity(abs_arc.clone(), evaluator.clone(), 60_000_000);
    let fp = AbstractionFingerprint::from_constants(200);
    trainer
        .load_checkpoint(&out("train.ckpt"), &fp)
        .expect("load checkpoint");
    println!("checkpoint loaded, iteration={}", trainer.iteration());

    let ev_ref: &dyn pkr_contracts::Evaluator = evaluator.as_ref();
    let abs_ref: &dyn AbstractionBuilder = abs_arc.as_ref();
    let tbl_ref = trainer.get_table();

    // ---- Run 1: CFR only (no blueprint) ----
    let cfg_cfr = POCConfig {
        root: river_root(&b),
        p0_range: Range::uniform(p0_hands.clone()),
        p1_range: Range::uniform(p1_hands.clone()),
        iterations: 200,
        evaluator: ev_ref,
        blueprint: None,
    };
    let t0 = std::time::Instant::now();
    let r_cfr = run_poc(&cfg_cfr);
    let dt_cfr = t0.elapsed();
    println!();
    println!("=== CFR (200 iters, no blueprint) ===");
    println!("  wall time:    {:.2}s", dt_cfr.as_secs_f64());
    println!("  nodes:        {}", r_cfr.nodes_visited);
    println!("  BR_v1:        {:.4} chips", r_cfr.br_v1_vs_cfr);

    // ---- Run 2: blueprint P0 ----
    let cfg_bp = POCConfig {
        root: river_root(&b),
        p0_range: Range::uniform(p0_hands),
        p1_range: Range::uniform(p1_hands),
        iterations: 0,  // no CFR solve
        evaluator: ev_ref,
        blueprint: Some((abs_ref, tbl_ref)),
    };
    let t1 = std::time::Instant::now();
    let r_bp = run_poc(&cfg_bp);
    let dt_bp = t1.elapsed();
    println!();
    println!("=== Blueprint (v34long 100M-iter) ===");
    println!("  wall time:    {:.2}s", dt_bp.as_secs_f64());
    println!("  BR_v1:        {:.4} chips", r_bp.br_v1_vs_blueprint.unwrap_or(f64::NAN));

    // ---- Compare ----
    let bp_val = r_bp.br_v1_vs_blueprint.unwrap_or(f64::NAN);
    println!();
    println!("=== RESULT ===");
    println!("  BR_v1 vs CFR:       {:.4} chips", r_cfr.br_v1_vs_cfr);
    println!("  BR_v1 vs blueprint: {:.4} chips", bp_val);
    let delta = bp_val - r_cfr.br_v1_vs_cfr;
    println!("  delta (BP - CFR):   {:+.4} chips", delta);
    if delta > 0.0 {
        println!("  => CFR beats blueprint by {:.2} chips", delta);
    } else {
        println!("  => Blueprint beats CFR by {:.2} chips", -delta);
    }
}
