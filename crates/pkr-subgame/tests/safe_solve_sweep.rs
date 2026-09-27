use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_subgame::range_tracker::{RangeTracker, sample_hands_weighted};
use pkr_subgame::{safe_solve, POCConfig, Range};
use std::sync::Arc;

fn ws() -> std::path::PathBuf {
    let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    m.parent().unwrap().parent().unwrap().to_path_buf()
}
fn out(rel: &str) -> String {
    ws().join("outputs/v34long").join(rel).to_string_lossy().into_owned()
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

fn forced_river<'a>(
    board: &[u8; 5],
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
    t.advance_street(&board[4..5]).ok()?;
    Some(t)
}

#[test]
#[ignore]
fn safe_solve_sweep() {
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
    let n_hands: usize = std::env::var("PKR_POC_HANDS").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    let iters: u32 = std::env::var("PKR_POC_ITERS").ok().and_then(|s| s.parse().ok()).unwrap_or(100);

    println!();
    println!("=== SAFE SOLVE SWEEP ({} boards, {} iters, {} hands) ===", n_boards, iters, n_hands);
    println!();
    println!("{:>6} {:>10} {:>10} {:>10} {:>10} {:>8}",
             "board", "cfr_br", "bp_br", "final_br", "alpha", "safe");
    println!("{}", "-".repeat(62));

    let mut alphas = Vec::new();
    let mut final_brs = Vec::new();
    let mut cfr_brs = Vec::new();
    let mut bp_brs = Vec::new();

    for seed in 0..n_boards {
        let b = board_for(seed);
        let tracker = match forced_river(&b, abs_ref, tbl_ref, ev_ref) {
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

        let r = safe_solve(&cfg);
        println!("{:>6} {:>10.4} {:>10.4} {:>10.4} {:>10.4} {:>8}",
                 seed, r.cfr_br, r.blueprint_br, r.final_br, r.alpha,
                 if r.safe { "yes" } else { "NO" });

        alphas.push(r.alpha);
        final_brs.push(r.final_br);
        cfr_brs.push(r.cfr_br);
        bp_brs.push(r.blueprint_br);
    }

    println!("{}", "-".repeat(62));
    println!();
    let n = alphas.len();
    if n == 0 { println!("  NO BOARDS"); return; }

    fn mean(v: &[f64]) -> f64 { v.iter().sum::<f64>() / v.len() as f64 }

    println!("=== SUMMARY (n={}) ===", n);
    println!("  mean cfr_br:       {:+.4}", mean(&cfr_brs));
    println!("  mean bp_br:        {:+.4}", mean(&bp_brs));
    println!("  mean final_br:     {:+.4}", mean(&final_brs));
    println!("  mean alpha:        {:.4}", mean(&alphas));
    println!("  boards where CFR was already safe (alpha=1.0): {}",
             alphas.iter().filter(|&&a| a > 0.99).count());
    println!("  boards needing full blend (alpha=0): {}",
             alphas.iter().filter(|&&a| a < 0.01).count());
    println!();
    let imp = mean(&bp_brs) - mean(&final_brs);
    println!("  Safe-solve improvement over blueprint: {:+.4} chips", imp);
    if imp > 10.0 {
        println!("  VERDICT: Safe solving retains most of the CFR win.");
    } else if imp > 3.0 {
        println!("  VERDICT: Safe solving retains some win.");
    } else {
        println!("  VERDICT: Safe solving gives up most of the win on these lines.");
    }
}
