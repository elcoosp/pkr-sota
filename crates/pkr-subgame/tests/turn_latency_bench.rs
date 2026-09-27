use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_contracts::AbstractionBuilder;
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

fn turn_board(seed: u64) -> [u8; 4] {
    let mut s = seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(0xBEEF);
    let mut used = [false; 52];
    let mut b = [0u8; 4];
    for i in 0..4 {
        loop {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let c = ((s >> 33) % 52) as u8;
            if !used[c as usize] { used[c as usize] = true; b[i] = c; break; }
        }
    }
    b
}

fn forced_turn<'a>(
    board: &[u8; 4],
    abs: &'a dyn AbstractionBuilder,
    tbl: &'a pkr_cfr::table::CompactRegretTable,
    ev: &'a dyn pkr_contracts::Evaluator,
) -> Option<RangeTracker<'a>> {
    let mut t = RangeTracker::new(GameState::new(200.0, 1.0, 2.0), abs, tbl, ev);
    let bet = |s: &GameState| Action { player: s.actor, kind: ActionKind::Bet(s.pot * 0.75) };
    let s0 = t.state().clone();
    t.apply_action(bet(&s0)).ok()?;
    t.apply_action(Action { player: 1, kind: ActionKind::Call }).ok()?;
    t.advance_street(&board[0..3]).ok()?;
    let s1 = t.state().clone();
    t.apply_action(bet(&s1)).ok()?;
    t.apply_action(Action { player: 1, kind: ActionKind::Call }).ok()?;
    t.advance_street(&board[3..4]).ok()?;
    t.apply_action(Action { player: 0, kind: ActionKind::Check }).ok()?;
    t.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;
    Some(t)
}

#[test]
#[ignore]
fn turn_latency_bench() {
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

    let b = turn_board(0);
    let tracker = forced_turn(&b, abs_ref, tbl_ref, ev_ref).expect("turn");
    let p0_s = sample_hands_weighted(tracker.range(0), 10, 0x1111);
    let p1_s = sample_hands_weighted(tracker.range(1), 10, 0x2222);

    let mode = if std::env::var("PKR_SUBGAME_FULL_CHANCE").ok().as_deref() == Some("0") {
        "sampled (MCCFR)"
    } else {
        "full_chance (CFR+)"
    };
    println!();
    println!("=== TURN LATENCY BENCH board={:?}, mode={} ===", b, mode);
    println!();
    println!("{:>6} {:>10} {:>12} {:>10}", "iters", "wall_s", "nodes", "br_v1");
    println!("{}", "-".repeat(44));

    for iters in [25, 50, 100, 200, 500] {
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
            iterations: iters,
            evaluator: ev_ref,
            blueprint: None,
        };
        let t0 = std::time::Instant::now();
        let r = run_poc(&cfg);
        let dt = t0.elapsed();
        println!("{:>6} {:>10.3} {:>12} {:>10.4}",
                 iters, dt.as_secs_f64(), r.nodes_visited, r.br_v1_vs_cfr);
    }
}
