//! Real-range POC sweep: play a realistic line through the blueprint,
//! then measure CFR-vs-blueprint at the river root using the tracker's
//! posterior ranges instead of uniform.
//!
//! This is the go/no-go gate for the subgame-solving direction.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_subgame::range_tracker::{RangeTracker, sample_hands_weighted};
use pkr_subgame::{run_poc, POCConfig, Range};
use std::sync::Arc;

fn workspace_root() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().to_path_buf()
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
            if !used[c as usize] {
                used[c as usize] = true;
                b[i] = c;
                break;
            }
        }
    }
    b
}

/// Force a specific line to the river root, updating the tracker's
/// ranges at every action. Line: preflop call+check, then check-check on
/// flop, turn, river. Matches the uniform POC so the delta reflects only
/// "range-informed vs uniform", not a line change.
fn play_to_river<'a>(
    board: &[u8; 5],
    abs: &'a dyn AbstractionBuilder,
    tbl: &'a pkr_cfr::table::CompactRegretTable,
    evaluator: &'a dyn pkr_contracts::Evaluator,
    line: &str,
) -> Option<RangeTracker<'a>> {
    let root = GameState::new(200.0, 1.0, 2.0);
    let mut tracker = RangeTracker::new(root, abs, tbl, evaluator);

    // Pot-sized bet relative to current street. The exact sizing doesn't
    // matter for the tracker — only the resulting bucket, which is
    // controlled by the blueprint's action_bucket mapping.
    let bet = |s: &GameState| Action {
        player: s.actor,
        kind: ActionKind::Bet(s.pot * 0.75),
    };

    match line {
        "passive" => {
            // call-check / check-check / check-check
            tracker.apply_action(Action { player: 0, kind: ActionKind::Call }).ok()?;
            tracker.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;
            tracker.advance_street(&board[0..3]).ok()?;
            tracker.apply_action(Action { player: 0, kind: ActionKind::Check }).ok()?;
            tracker.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;
            tracker.advance_street(&board[3..4]).ok()?;
            tracker.apply_action(Action { player: 0, kind: ActionKind::Check }).ok()?;
            tracker.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;
        }
        "aggressive" => {
            // preflop: SB raise, BB call
            let s0 = tracker.state().clone();
            tracker.apply_action(bet(&s0)).ok()?;
            tracker.apply_action(Action { player: 1, kind: ActionKind::Call }).ok()?;
            // flop: SB bet, BB call
            tracker.advance_street(&board[0..3]).ok()?;
            let s1 = tracker.state().clone();
            tracker.apply_action(bet(&s1)).ok()?;
            tracker.apply_action(Action { player: 1, kind: ActionKind::Call }).ok()?;
            // turn: check-check
            tracker.advance_street(&board[3..4]).ok()?;
            tracker.apply_action(Action { player: 0, kind: ActionKind::Check }).ok()?;
            tracker.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;
        }
        other => panic!("unknown line: {}", other),
    }

    // River root
    tracker.advance_street(&board[4..5]).ok()?;
    Some(tracker)
}

#[test]
#[ignore]
fn real_range_sweep() {
    let abs_a = {
        let centroids = load_centroids(&out("centroids.bin")).expect("centroids");
        let abs = KMeansAbstraction::from_store(centroids, Arc::new(pkr_eval::NlheEvaluator));
        abs.init_table(0, &out("preflop_abstraction.bin")).expect("t0");
        abs.init_table(1, &out("abstraction.bin")).expect("t1");
        abs.init_table(2, &out("turn_abstraction.bin")).expect("t2");
        abs.init_table(3, &out("river_buckets.bin")).expect("t3");
        Arc::new(abs)
    };
    let evaluator = Arc::new(pkr_eval::NlheEvaluator);
    let abs_dyn: Arc<dyn AbstractionBuilder> = abs_a.clone();
    let trainer = Trainer::with_capacity(abs_dyn, evaluator.clone(), 60_000_000);
    let fp = AbstractionFingerprint::from_constants(200);
    trainer.load_checkpoint(&out("train.ckpt"), &fp).expect("ckpt");

    let abs_ref: &dyn AbstractionBuilder = abs_a.as_ref();
    let ev_ref: &dyn pkr_contracts::Evaluator = evaluator.as_ref();
    let tbl_ref = trainer.get_table();

    let line: String = std::env::var("PKR_POC_LINE")
        .unwrap_or_else(|_| "passive".to_string());
    println!("  line = {}", line);

    let n_boards: u64 = std::env::var("PKR_POC_BOARDS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(20);
    let n_hands: usize = std::env::var("PKR_POC_HANDS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    let iters: u32 = std::env::var("PKR_POC_ITERS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(100);

    let mut deltas: Vec<f64> = Vec::new();
    let mut wide_deltas: Vec<f64> = Vec::new();
    let mut ratios: Vec<f64> = Vec::new();
    let mut fails = 0usize;

    println!();
    println!("{:>6} {:>24} {:>10} {:>10} {:>10} {:>8} {:>6} {:>10} {:>10} {:>10}",
             "board", "cards", "cfr", "bp", "delta", "ratio", "h_p1",
             "cfr_w", "bp_w", "delta_w");
    println!("{}", "-".repeat(80));

    for seed in 0..n_boards {
        let b = board_for(seed);
        let tracker = match play_to_river(&b, abs_ref, tbl_ref, ev_ref, &line) {
            Some(t) => t,
            None => { fails += 1; continue; }
        };
        tracker.assert_normalized().ok();

        let p0_samples = sample_hands_weighted(tracker.range(0), n_hands, seed ^ 0x1111);
        let p1_samples = sample_hands_weighted(tracker.range(1), n_hands, seed ^ 0x2222);
        if p0_samples.len() < n_hands / 2 || p1_samples.len() < n_hands / 2 {
            fails += 1; continue;
        }

        let p1_entropy: f64 = -tracker.range(1).iter()
            .filter(|&&v| v > 1e-12)
            .map(|&v| v * v.ln())
            .sum::<f64>();

        let p0_hands: Vec<[u8; 2]> = p0_samples.iter().map(|(h, _)| *h).collect();
        let p1_hands: Vec<[u8; 2]> = p1_samples.iter().map(|(h, _)| *h).collect();

        let cfg_cfr = POCConfig {
            root: tracker.state().clone(),
            p0_range: Range::uniform(p0_hands.clone()),
            p1_range: Range::uniform(p1_hands.clone()),
            iterations: iters,
            evaluator: ev_ref,
            blueprint: None,
        };
        let r_cfr = run_poc(&cfg_cfr);

        let cfg_bp = POCConfig {
            root: tracker.state().clone(),
            p0_range: Range::uniform(p0_hands),
            p1_range: Range::uniform(p1_hands),
            iterations: 0,
            evaluator: ev_ref,
            blueprint: Some((abs_ref, tbl_ref)),
        };
        let r_bp = run_poc(&cfg_bp);

        let bp_val = r_bp.br_v1_vs_blueprint.unwrap_or(f64::NAN);
        let delta = bp_val - r_cfr.br_v1_vs_cfr;
        let ratio = if bp_val.abs() > 1e-9 { r_cfr.br_v1_vs_cfr / bp_val } else { f64::NAN };

        deltas.push(delta);
        ratios.push(ratio);

        let cfr_wide = r_cfr.br_v1_vs_cfr_wide;
        let bp_wide = r_bp.br_v1_vs_blueprint_wide.unwrap_or(f64::NAN);
        let delta_wide = bp_wide - cfr_wide;
        wide_deltas.push(delta_wide);
        println!("{:>6} {:>24} {:>10.4} {:>10.4} {:>+10.4} {:>8.3} {:>6.2} {:>10.4} {:>10.4} {:>+10.4}",
                 seed, format!("{:?}", b),
                 r_cfr.br_v1_vs_cfr, bp_val, delta, ratio, p1_entropy,
                 cfr_wide, bp_wide, delta_wide);
    }

    println!("{}", "-".repeat(80));
    println!();
    let n = deltas.len();
    if n == 0 {
        println!("  NO VALID BOARDS (fails={})", fails);
        return;
    }
    deltas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean_delta: f64 = deltas.iter().sum::<f64>() / n as f64;
    let median_delta = if n % 2 == 1 { deltas[n / 2] } else { (deltas[n / 2 - 1] + deltas[n / 2]) / 2.0 };
    let mean_ratio: f64 = ratios.iter().sum::<f64>() / n as f64;
    let wins = deltas.iter().filter(|&&d| d > 0.0).count();

    println!("=== REAL-RANGE SWEEP (n={}, {} iters, {} hands/range) ===", n, iters, n_hands);
    println!("  fails:          {}", fails);
    println!("  wins / total:   {}/{}", wins, n);
    println!("  mean delta:     {:+.4} chips", mean_delta);
    println!("  median delta:   {:+.4} chips", median_delta);
    println!("  mean ratio:     {:.4}", mean_ratio);
    println!("  min delta:      {:+.4}", deltas[0]);
    println!("  max delta:      {:+.4}", deltas[n - 1]);
    if !wide_deltas.is_empty() {
        wide_deltas.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let nw = wide_deltas.len();
        let wmean: f64 = wide_deltas.iter().sum::<f64>() / nw as f64;
        let wmed = if nw % 2 == 1 { wide_deltas[nw/2] } else { (wide_deltas[nw/2-1] + wide_deltas[nw/2]) / 2.0 };
        let wwins = wide_deltas.iter().filter(|&&d| d > 0.0).count();
        println!();
        println!("=== WIDE-RANGE (opponent deviates to uniform prior) ===");
        println!("  wins:          {}/{}", wwins, nw);
        println!("  mean delta:    {:+.4}", wmean);
        println!("  median delta:  {:+.4}", wmed);
        println!("  min delta:     {:+.4}", wide_deltas[0]);
        println!("  max delta:     {:+.4}", wide_deltas[nw-1]);
        println!();
        if wmed > 10.0 {
            println!("  -> CFR win is ROBUST to opponent range deviation.");
            println!("     No safe-solving gadget needed for these lines.");
        } else if wmed > 0.0 {
            println!("  -> CFR win weakens under deviation; gadget advisable.");
        } else {
            println!("  -> CFR can be EXPLOITED by opponent deviation;");
            println!("     max-margin gadget is mandatory before deployment.");
        }
    }

    println!();
    println!("  Compare to uniform-range POC:");
    println!("    uniform median delta:  +20.50 chips");
    println!("    uniform median ratio:  0.001");
    println!();
    if median_delta > 10.0 {
        println!("  VERDICT: STRONG — proceed with full build");
    } else if median_delta > 4.0 {
        println!("  VERDICT: MARGINAL — river-only MVP worth it");
    } else if median_delta > 1.0 {
        println!("  VERDICT: WEAK — not worth 2 weeks");
    } else {
        println!("  VERDICT: NEGATIVE — blueprint already near-optimal on real ranges");
    }
}
