use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_subgame::range_tracker::{RangeTracker, sample_hands_weighted};
use pkr_subgame::{run_poc, POCConfig, Range};
use std::sync::Arc;

fn ws() -> std::path::PathBuf {
    let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    m.parent().unwrap().parent().unwrap().to_path_buf()
}
fn out(rel: &str) -> String {
    ws().join("outputs/v34long").join(rel).to_string_lossy().into_owned()
}
fn board() -> [u8; 5] { [48, 37, 19, 26, 12] }

fn forced<'a>(
    b: &[u8; 5], abs: &'a dyn AbstractionBuilder,
    tbl: &'a pkr_cfr::table::CompactRegretTable,
    ev: &'a dyn pkr_contracts::Evaluator,
) -> Option<RangeTracker<'a>> {
    let mut t = RangeTracker::new(GameState::new(200.0, 1.0, 2.0), abs, tbl, ev);
    let bet = |s: &GameState| Action { player: s.actor, kind: ActionKind::Bet(s.pot * 0.75) };
    let s0 = t.state().clone();
    t.apply_action(bet(&s0)).ok()?;
    t.apply_action(Action { player: 1, kind: ActionKind::Call }).ok()?;
    t.advance_street(&b[0..3]).ok()?;
    let s1 = t.state().clone();
    t.apply_action(bet(&s1)).ok()?;
    t.apply_action(Action { player: 1, kind: ActionKind::Call }).ok()?;
    t.advance_street(&b[3..4]).ok()?;
    t.apply_action(Action { player: 0, kind: ActionKind::Check }).ok()?;
    t.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;
    t.advance_street(&b[4..5]).ok()?;
    Some(t)
}

#[test]
#[ignore]
fn cfr_strategy_varies_by_hand() {
    let abs_a = {
        let c = load_centroids(&out("centroids.bin")).expect("centroids");
        let a = KMeansAbstraction::from_store(c, Arc::new(pkr_eval::NlheEvaluator));
        a.init_table(0, &out("preflop_abstraction.bin")).unwrap();
        a.init_table(1, &out("abstraction.bin")).unwrap();
        a.init_table(2, &out("turn_abstraction.bin")).unwrap();
        a.init_table(3, &out("river_buckets.bin")).unwrap();
        Arc::new(a)
    };
    let evaluator = Arc::new(pkr_eval::NlheEvaluator);
    let abs_dyn: Arc<dyn AbstractionBuilder> = abs_a.clone();
    let trainer = Trainer::with_capacity(abs_dyn, evaluator.clone(), 60_000_000);
    let fp = AbstractionFingerprint::from_constants(200);
    trainer.load_checkpoint(&out("train.ckpt"), &fp).expect("ckpt");

    let abs_ref: &dyn AbstractionBuilder = abs_a.as_ref();
    let ev_ref: &dyn pkr_contracts::Evaluator = evaluator.as_ref();
    let tbl_ref = trainer.get_table();

    let b = board();
    let tracker = forced(&b, abs_ref, tbl_ref, ev_ref).expect("line");
    let p0_s = sample_hands_weighted(tracker.range(0), 24, 0x1111);
    let p1_s = sample_hands_weighted(tracker.range(1), 24, 0x2222);

    let cfg = POCConfig {
        root: tracker.state().clone(),
        p0_range: Range::weighted(
            p0_s.iter().map(|(h, _)| *h).collect(),
            p0_s.iter().map(|(_, p)| *p).collect(),
        ),
        p1_range: Range::weighted(
            p1_s.iter().map(|(h, _)| *h).collect(),
            p1_s.iter().map(|(_, p)| *p).collect(),
        ),
        iterations: 200,
        evaluator: ev_ref,
        blueprint: None,
    };

    // Use run_poc to build the solver internally, then... we need access.
    // Workaround: use root_strategies-like helper — but the diagnostic
    // helper is on Solver. Since Solver isn't public, we go through
    // run_poc and check the returned POCResult. Actually we can't.
    //
    // Simplest path: use a lower-level public fn.
    //
    // We'll add a public diagnostics entrypoint below.
    let _ = run_poc(&cfg); // ensure cfg works
    // Direct approach: use the exposed diagnostics fn (below).

    let diag = pkr_subgame::diagnose_strategy_variance(&cfg);
    println!();
    println!("=== CFR STRATEGY VARIANCE (board={:?}) ===", b);
    println!("  p0 decision nodes:         {}", diag.p0_decision_nodes);
    println!("  distinct root strategies:  {}", diag.distinct_strategies);
    println!("  total variance across deals: {:.6}", diag.total_variance);
    println!();
    if let Some((node_id, strategies)) = diag.first_node_strategies {
        println!("  first P0 decision node: {}", node_id);
        println!();
        println!("  {:>4} {:>12} {:>16}  strategy", "idx", "rank", "p0_hand");
        println!("  {}", "-".repeat(70));
        for (i, (h0, s)) in diag.p0_hands.iter().zip(strategies.iter()).enumerate() {
            let rank = evaluator.evaluate_hand(h0, &b);
            let ss: String = s.iter().enumerate()
                .filter(|(_, &p)| p > 1e-6)
                .map(|(bi, p)| format!("b{}:{:.3}", bi, p))
                .collect::<Vec<_>>().join(" ");
            println!("  {:>4} {:>12} {:>16}  {}", i, rank, format!("{:?}", h0), ss);
        }
    }
    println!();
    if diag.distinct_strategies > 1 && diag.total_variance > 1e-6 {
        println!("  VERDICT: CFR VARY by hand -> real Nash-style solution.");
    } else {
        println!("  VERDICT: IDENTICAL -> CFR did not learn. Adversarial result artefact.");
    }
}
