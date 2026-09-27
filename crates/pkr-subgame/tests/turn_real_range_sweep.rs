//! Turn real-range sweep: does the CFR turn solver beat the blueprint
//! the way the river solver did?
//!
//! Forced line: preflop raise-call, flop bet-call, turn check-check.
//! At the turn root, sample hands from the tracker's posterior, solve
//! with the turn solver, compare BR_v1 vs blueprint.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState, Street};
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

fn turn_board_for(seed: u64) -> [u8; 4] {
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

fn forced_turn_line<'a>(
    board: &[u8; 4],
    abs: &'a dyn AbstractionBuilder,
    tbl: &'a pkr_cfr::table::CompactRegretTable,
    ev: &'a dyn pkr_contracts::Evaluator,
) -> Option<RangeTracker<'a>> {
    let mut t = RangeTracker::new(GameState::new(200.0, 1.0, 2.0), abs, tbl, ev);
    let bet = |s: &GameState| Action { player: s.actor, kind: ActionKind::Bet(s.pot * 0.75) };

    // Preflop: SB raise, BB call
    let s0 = t.state().clone();
    t.apply_action(bet(&s0)).ok()?;
    t.apply_action(Action { player: 1, kind: ActionKind::Call }).ok()?;

    // Flop
    t.advance_street(&board[0..3]).ok()?;
    let s1 = t.state().clone();
    t.apply_action(bet(&s1)).ok()?;
    t.apply_action(Action { player: 1, kind: ActionKind::Call }).ok()?;

    // Turn: check-check -> at turn complete (root for our solve)
    t.advance_street(&board[3..4]).ok()?;
    t.apply_action(Action { player: 0, kind: ActionKind::Check }).ok()?;
    t.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;

    Some(t)
}

#[test]
#[ignore]
fn turn_real_range_sweep() {
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

    let n_boards: u64 = std::env::var("PKR_POC_BOARDS").ok().and_then(|s| s.parse().ok()).unwrap_or(10);
    let n_hands: usize = std::env::var("PKR_POC_HANDS").ok().and_then(|s| s.parse().ok()).unwrap_or(10);
    let iters: u32 = std::env::var("PKR_POC_ITERS").ok().and_then(|s| s.parse().ok()).unwrap_or(50);

    println!();
    println!("=== TURN REAL-RANGE SWEEP ({} boards, {} iters, {} hands) ===", n_boards, iters, n_hands);
    println!("line: SB raise, BB call; SB bet, BB call; check-check on turn");
    println!();
    println!("{:>6} {:>20} {:>10} {:>10} {:>10}", "board", "cards", "cfr", "bp", "delta");
    println!("{}", "-".repeat(60));

    let mut deltas: Vec<f64> = Vec::new();
    let mut fails = 0;

    for seed in 0..n_boards {
        let b = turn_board_for(seed);
        let tracker = match forced_turn_line(&b, abs_ref, tbl_ref, ev_ref) {
            Some(t) => t,
            None => { fails += 1; continue; }
        };
        if tracker.state().street != Street::Turn {
            fails += 1;
            continue;
        }

        let p0_s = sample_hands_weighted(tracker.range(0), n_hands, seed ^ 0x1111);
        let p1_s = sample_hands_weighted(tracker.range(1), n_hands, seed ^ 0x2222);
        if p0_s.len() < n_hands / 2 || p1_s.len() < n_hands / 2 { fails += 1; continue; }

        let cfg_cfr = POCConfig {
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
        let r_cfr = run_poc(&cfg_cfr);

        let cfg_bp = POCConfig {
            root: tracker.state().clone(),
            p0_range: Range::weighted(
                p0_s.iter().map(|(h, _)| *h).collect(),
                p0_s.iter().map(|(_, p)| *p).collect(),
            ),
            p1_range: Range::weighted(
                p1_s.iter().map(|(h, _)| *h).collect(),
                p1_s.iter().map(|(_, p)| *p).collect(),
            ),
            iterations: 0,
            evaluator: ev_ref,
            blueprint: Some((abs_ref, tbl_ref)),
        };
        let r_bp = run_poc(&cfg_bp);

        let bp_val = r_bp.br_v1_vs_blueprint.unwrap_or(f64::NAN);
        let delta = bp_val - r_cfr.br_v1_vs_cfr;
        deltas.push(delta);

        println!("{:>6} {:>20} {:>10.4} {:>10.4} {:>+10.4}",
                 seed, format!("{:?}", b),
                 r_cfr.br_v1_vs_cfr, bp_val, delta);
    }

    println!("{}", "-".repeat(60));
    println!();
    if deltas.is_empty() { println!("  NO VALID BOARDS (fails={})", fails); return; }

    deltas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = deltas.len();
    let mean = deltas.iter().sum::<f64>() / n as f64;
    let med = if n % 2 == 1 { deltas[n/2] } else { (deltas[n/2-1] + deltas[n/2]) / 2.0 };
    let wins = deltas.iter().filter(|&&d| d > 0.0).count();

    println!("=== SUMMARY (n={}, fails={}) ===", n, fails);
    println!("  wins / total:  {}/{}", wins, n);
    println!("  mean delta:    {:+.4} chips", mean);
    println!("  median delta:  {:+.4} chips", med);
    println!("  min delta:     {:+.4}", deltas[0]);
    println!("  max delta:     {:+.4}", deltas[n-1]);
    println!();
    println!("  Compare to river sweep: median +42.96 chips (tracked)");
    if med > 15.0 {
        println!("  VERDICT: turn CFR win holds. Full 2-week build proceeds.");
    } else if med > 5.0 {
        println!("  VERDICT: turn CFR win weaker than river but positive.");
    } else {
        println!("  VERDICT: turn CFR win is small or negative. Investigate.");
    }
}
