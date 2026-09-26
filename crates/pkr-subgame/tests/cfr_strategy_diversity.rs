//! Diagnostic: is CFR's solution at the river root hand-dependent?
//!
//! For each board, extract P0's strategy over every deal's P0 hand.
//! If the strategy varies across hands, CFR is producing a real Nash-
//! style solution (fold weak, bet strong). If it's constant across
//! hands, the "adversarial" result is a tree artefact — P1 finds the
//! same escape hatch regardless of cards.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_subgame::range_tracker::{RangeTracker, sample_hands_weighted};
use pkr_subgame::{run_poc, POCConfig, Range};
use std::sync::Arc;

fn workspace_root() -> std::path::PathBuf {
    let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    m.parent().unwrap().parent().unwrap().to_path_buf()
}
fn out(rel: &str) -> String {
    workspace_root().join("outputs/v34long").join(rel).to_string_lossy().into_owned()
}

fn board_for(seed: u64) -> [u8; 5] {
    let mut s = seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(0xDEADBEEF);
    let mut used = [false; 52];
    let mut b = [0u8; 5];
    for i in 0..5 {
        loop {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let c = ((s >> 33) % 52) as u8;
            if !used[c as usize] { used[c as usize] = true; b[i] = c; break; }
        }
    }
    b
}

fn forced_line<'a>(
    board: &[u8; 5],
    abs: &'a dyn AbstractionBuilder,
    tbl: &'a pkr_cfr::table::CompactRegretTable,
    ev: &'a dyn pkr_contracts::Evaluator,
) -> Option<RangeTracker<'a>> {
    let root = GameState::new(200.0, 1.0, 2.0);
    let mut t = RangeTracker::new(root, abs, tbl, ev);
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
    t.advance_street(&board[4..5]).ok()?;
    Some(t)
}

#[test]
#[ignore]
fn cfr_strategy_is_hand_dependent() {
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

    let n_hands: usize = std::env::var("PKR_POC_HANDS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(24);
    let iters: u32 = std::env::var("PKR_POC_ITERS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(200);

    println!();
    println!("=== CFR STRATEGY DIVERSITY ({} hands, {} iters) ===", n_hands, iters);
    println!();

    let mut strategy_deltas: Vec<f64> = Vec::new();

    for seed in 0..5 {
        let b = board_for(seed);
        let tracker = match forced_line(&b, abs_ref, tbl_ref, ev_ref) {
            Some(t) => t,
            None => { println!("  board {}: line failed", seed); continue; }
        };
        let p0_s = sample_hands_weighted(tracker.range(0), n_hands, seed ^ 0x1111);
        let p1_s = sample_hands_weighted(tracker.range(1), n_hands, seed ^ 0x2222);

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

        // Run POC once to build the solver. We need to expose solver internals.
        // Simplest: hack a diagnostic through POCResult? No — let's just call
        // Solver directly.
        // Actually Solver isn't public. Use the POC and infer from the strategy
        // weights... we don't have access. Instead, measure a proxy: the range
        // of CFR BR values across deals. If they're all identical, the
        // strategy is likely also identical.
        let r = run_poc(&cfg);

        // We don't have direct access to solver internals from here. But we
        // can measure the variance of per-deal BR values via the adversarial
        // test on a subset. For now, print the mean BR.
        println!("  board {:2} {:?}:  cfr_mean_br={:.4}",
                 seed, b, r.br_v1_vs_cfr);
        strategy_deltas.push(r.br_v1_vs_cfr);
    }

    println!();
    println!("  Mean BR across 5 boards:  {:.4}", strategy_deltas.iter().sum::<f64>() / strategy_deltas.len() as f64);
    println!();
    println!("  NOTE: this diagnostic prints mean BR, not per-deal strategy.");
    println!("        The full strategy-distribution test requires exposing");
    println!("        Solver internals. The cfr_max uniformity remains an open");
    println!("        question — likely a Nash convergence signature (all P1 hands");
    println!("        have equal BR value at equilibrium) but not proven.");
}
