//! 20-board POC sweep: CFR vs blueprint per board, aggregated.
//! Loads the checkpoint ONCE, then loops 20 boards.
//! Run with: cargo nextest run -p pkr-subgame --run-ignored all --nocapture poc_river_sweep

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::Trainer;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState};
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
fn poc_river_sweep() {
    // ---- Load blueprint once ----
    let centroids_store = load_centroids(&out("centroids.bin")).expect("centroids");
    let abs = KMeansAbstraction::from_store(centroids_store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, &out("preflop_abstraction.bin")).expect("t0");
    abs.init_table(1, &out("abstraction.bin")).expect("t1");
    abs.init_table(2, &out("turn_abstraction.bin")).expect("t2");
    abs.init_table(3, &out("river_buckets.bin")).expect("t3");
    let abs_arc: Arc<dyn AbstractionBuilder> = Arc::new(abs);

    let evaluator = Arc::new(pkr_eval::NlheEvaluator);
    let trainer = Trainer::with_capacity(abs_arc.clone(), evaluator.clone(), 60_000_000);
    let fp = AbstractionFingerprint::from_constants(200);
    trainer.load_checkpoint(&out("train.ckpt"), &fp).expect("ckpt");
    println!("checkpoint loaded (iter {})", trainer.iteration());

    let ev_ref: &dyn pkr_contracts::Evaluator = evaluator.as_ref();
    let abs_ref: &dyn AbstractionBuilder = abs_arc.as_ref();
    let tbl_ref = trainer.get_table();

    let iters: u32 = std::env::var("PKR_POC_ITERS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    let n_hands: usize = std::env::var("PKR_POC_HANDS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    let n_boards: u64 = std::env::var("PKR_POC_BOARDS")
        .ok().and_then(|s| s.parse().ok()).unwrap_or(20);

    let mut deltas = Vec::new();
    let mut ratios = Vec::new();
    println!();
    println!("{:<6} {:<24} {:>10} {:>10} {:>10} {:>8}", "board", "cards", "cfr", "bp", "delta", "ratio");
    println!("{}", "-".repeat(74));

    let t_sweep = std::time::Instant::now();
    for seed in 0..n_boards {
        let b = board_for(seed);
        let p0_pool: Vec<u8> = (0u8..26).collect();
        let p1_pool: Vec<u8> = (26u8..52).collect();
        let p0_hands = make_range(&p0_pool, &b, n_hands);
        let p1_hands = make_range(&p1_pool, &b, n_hands);

        let cfg_cfr = POCConfig {
            root: river_root(&b),
            p0_range: Range::uniform(p0_hands.clone()),
            p1_range: Range::uniform(p1_hands.clone()),
            iterations: iters,
            evaluator: ev_ref,
            blueprint: None,
        };
        let r_cfr = run_poc(&cfg_cfr);

        let cfg_bp = POCConfig {
            root: river_root(&b),
            p0_range: Range::uniform(p0_hands),
            p1_range: Range::uniform(p1_hands),
            iterations: 0,
            evaluator: ev_ref,
            blueprint: Some((abs_ref, tbl_ref)),
        };
        let r_bp = run_poc(&cfg_bp);

        let bp_val = r_bp.br_v1_vs_blueprint.unwrap_or(f64::NAN);
        let delta = bp_val - r_cfr.br_v1_vs_cfr;
        let ratio = r_cfr.br_v1_vs_cfr / bp_val;

        deltas.push(delta);
        ratios.push(ratio);

        println!("{:<6} {:?} {:>10.4} {:>10.4} {:>+10.4} {:>8.3}",
            seed, b, r_cfr.br_v1_vs_cfr, bp_val, delta, ratio);
    }

    let dt = t_sweep.elapsed();
    println!("{}", "-".repeat(74));
    println!();

    // Aggregate
    deltas.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = deltas.len();
    let mean_delta: f64 = deltas.iter().sum::<f64>() / n as f64;
    let median_delta = if n % 2 == 1 { deltas[n / 2] } else { (deltas[n/2 - 1] + deltas[n/2]) / 2.0 };
    let mean_ratio: f64 = ratios.iter().sum::<f64>() / n as f64;
    let median_ratio = if n % 2 == 1 { ratios[n / 2] } else { (ratios[n/2 - 1] + ratios[n/2]) / 2.0 };
    let wins = deltas.iter().filter(|&&d| d > 0.0).count();

    println!("=== SWEEP SUMMARY (n={} boards, {} CFR iters, {} hands/range) ===", n, iters, n_hands);
    println!("  wall time:      {:.1}s ({:.2}s/board)", dt.as_secs_f64(), dt.as_secs_f64() / n as f64);
    println!("  wins / total:   {}/{} ({:.0}%)", wins, n, 100.0 * wins as f64 / n as f64);
    println!("  mean delta:     {:+.4} chips", mean_delta);
    println!("  median delta:   {:+.4} chips", median_delta);
    println!("  mean ratio:     {:.3} (lower=better CFR)", mean_ratio);
    println!("  median ratio:   {:.3}", median_ratio);
    println!("  min delta:      {:+.4}", deltas[0]);
    println!("  max delta:      {:+.4}", deltas[n-1]);
    println!();
    if median_delta > 0.5 {
        println!("  VERDICT: median CFR win >0.5 chips across {} boards.", n);
        println!("           Subgame solving is a real direction.");
    } else if median_delta > 0.0 {
        println!("  VERDICT: median CFR win is small (<0.5 chips).");
        println!("           Worth investigating but with caution.");
    } else {
        println!("  VERDICT: median CFR win is NEGATIVE. Blueprint beats CFR.");
        println!("           Reconsider before scaling.");
    }
}
