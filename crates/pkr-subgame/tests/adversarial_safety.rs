//! Adversarial safety test: P1 picks the deal that maximizes BR value
//! against the CFR strategy. Measures worst-case exploitability.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_subgame::range_tracker::{RangeTracker, sample_hands_weighted};
use pkr_subgame::{adversarial_safety_test, POCConfig, Range};
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
    line: &str,
) -> Option<RangeTracker<'a>> {
    let root = GameState::new(200.0, 1.0, 2.0);
    let mut t = RangeTracker::new(root, abs, tbl, ev);
    let bet = |s: &GameState| Action { player: s.actor, kind: ActionKind::Bet(s.pot * 0.75) };
    match line {
        "passive" => {
            t.apply_action(Action { player: 0, kind: ActionKind::Call }).ok()?;
            t.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;
            t.advance_street(&board[0..3]).ok()?;
            t.apply_action(Action { player: 0, kind: ActionKind::Check }).ok()?;
            t.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;
            t.advance_street(&board[3..4]).ok()?;
            t.apply_action(Action { player: 0, kind: ActionKind::Check }).ok()?;
            t.apply_action(Action { player: 1, kind: ActionKind::Check }).ok()?;
        }
        "aggressive" => {
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
        }
        _ => panic!("unknown line"),
    }
    t.advance_street(&board[4..5]).ok()?;
    Some(t)
}

#[test]
#[ignore]
fn adversarial_safety_sweep() {
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

    let line = std::env::var("PKR_POC_LINE").unwrap_or_else(|_| "aggressive".into());
    let n_boards: u64 = std::env::var("PKR_POC_BOARDS").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
    let n_hands: usize = std::env::var("PKR_POC_HANDS").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    let iters: u32 = std::env::var("PKR_POC_ITERS").ok().and_then(|s| s.parse().ok()).unwrap_or(100);

    println!();
    println!("=== ADVERSARIAL SAFETY (line={}, {} iters, {} hands) ===", line, iters, n_hands);
    println!();
    println!("{:>6} {:>24} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
             "board", "cards", "cfr_mean", "cfr_max", "cfr_p95",
             "bp_mean", "bp_max", "bp_p95");
    println!("{}", "-".repeat(96));

    let mut cfr_means = Vec::new();
    let mut cfr_maxs = Vec::new();
    let mut bp_means = Vec::new();
    let mut bp_maxs = Vec::new();
    let mut cfr_wins_max = 0;
    let mut total = 0;

    for seed in 0..n_boards {
        let b = board_for(seed);
        let tracker = match forced_line(&b, abs_ref, tbl_ref, ev_ref, &line) {
            Some(t) => t,
            None => continue,
        };
        let p0_s = sample_hands_weighted(tracker.range(0), n_hands, seed ^ 0x1111);
        let p1_s = sample_hands_weighted(tracker.range(1), n_hands, seed ^ 0x2222);
        if p0_s.len() < n_hands / 2 || p1_s.len() < n_hands / 2 { continue; }

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
            blueprint: Some((abs_ref, tbl_ref)),
        };

        let r = adversarial_safety_test(&cfg);
        let bm = r.blueprint_mean.unwrap_or(f64::NAN);
        let bmx = r.blueprint_max.unwrap_or(f64::NAN);
        let bp95 = r.blueprint_p95.unwrap_or(f64::NAN);

        println!("{:>6} {:>24} {:>10.4} {:>10.4} {:>10.4} {:>10.4} {:>10.4} {:>10.4}",
                 seed, format!("{:?}", b),
                 r.cfr_mean, r.cfr_max, r.cfr_p95, bm, bmx, bp95);

        cfr_means.push(r.cfr_mean);
        cfr_maxs.push(r.cfr_max);
        bp_means.push(bm);
        bp_maxs.push(bmx);
        total += 1;
        if r.cfr_max < bmx { cfr_wins_max += 1; }
    }

    println!("{}", "-".repeat(96));
    println!();
    if total == 0 { println!("  NO BOARDS"); return; }

    fn mean(v: &[f64]) -> f64 { v.iter().sum::<f64>() / v.len() as f64 }
    fn med(v: &[f64]) -> f64 {
        let mut s = v.to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = s.len();
        if n % 2 == 1 { s[n/2] } else { (s[n/2-1] + s[n/2]) / 2.0 }
    }

    println!("=== SUMMARY (n={}) ===", total);
    println!();
    println!("  CFR strategy:");
    println!("    mean BR across deals (avg case):  {:+.4}", mean(&cfr_means));
    println!("    median worst-deal BR:            {:+.4}", med(&cfr_maxs));
    println!("    mean of per-board max BR:        {:+.4}", mean(&cfr_maxs));
    println!();
    println!("  Blueprint strategy:");
    println!("    mean BR across deals:            {:+.4}", mean(&bp_means));
    println!("    median worst-deal BR:            {:+.4}", med(&bp_maxs));
    println!("    mean of per-board max BR:        {:+.4}", mean(&bp_maxs));
    println!();
    println!("  CFR worst-deal < blueprint worst-deal: {}/{} boards",
             cfr_wins_max, total);
    println!();

    let cm = mean(&cfr_maxs);
    let bm = mean(&bp_maxs);
    let delta = bm - cm;
    println!("  ADVERSARIAL DELTA (BP - CFR): {:+.4} chips", delta);
    println!();
    if delta > 5.0 {
        println!("  VERDICT: PASS — CFR's worst deal is materially better than");
        println!("           the blueprint's. Adversarial play does not flip");
        println!("           the CFR advantage.");
    } else if delta > 0.0 {
        println!("  VERDICT: MARGINAL — CFR survives adversarial P1 but the");
        println!("           margin shrinks. Safe-solving worth adding.");
    } else {
        println!("  VERDICT: FAIL — P1 can flip the CFR advantage by choosing");
        println!("           the worst deal. Safe solving mandatory.");
    }
}
