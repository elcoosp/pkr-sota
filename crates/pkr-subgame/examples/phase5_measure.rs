//! Phase-5 step-1 measurement (2026-10-11).
//!
//! Design doc: docs/roadmap/phase-5-depth-limited-flop-design.md.
//! Times the TURN subtree build (4-card board, 46 rivers). Do NOT
//! build a flop tree here — it is 46x the turn tree and OOMs.
//!
//! Run with a hand-count cap so peak RSS is bounded:
//!   cargo run -p pkr-subgame --example phase5_measure -- <n_hands>
//!
//! n_hands controls the range width; the tree SHAPE is the same for
//! any range, so per-node cost is what we measure. Extrapolate to
//! 1326 hands by scaling the node count.

use pkr_subgame::vector::{Solver, Spec};

fn det_eval(_h: &[u8; 2], _b: &[u8]) -> u32 { 0 }

fn lcg(seed: &mut u64) -> f32 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*seed >> 33) as f32) / ((1u64 << 31) as f32)
}

fn build(board: Vec<u8>, n_hands: usize, seed: u64) -> Solver<'static> {
    let mut s = seed;
    let mut r0 = vec![0.0f32; 1326];
    let mut r1 = vec![0.0f32; 1326];
    // Give only `n_hands` distinct hands non-zero mass on each side,
    // so the tree is functionally identical but the ACTIVE set is
    // small. The tree shape does not depend on range widths.
    for h in 0..n_hands.min(1326) {
        r0[h] = lcg(&mut s);
        r1[h] = lcg(&mut s);
    }
    let spec = Spec {
        board,
        dead: 20.0,
        stack: 190.0,
        root_actor: 0,
        root_inv: [0.0, 0.0],
        bet_fracs: vec![0.5, 1.0, 2.0],
        max_raises: 3,
        min_bet: 2.0,
        allin_snap: 0.2,
        ranges: [r0, r1],
    };
    let eval: &'static (dyn Fn(&[u8; 2], &[u8]) -> u32 + Sync) = &det_eval;
    Solver::new(spec, eval)
}

fn rss_mb() -> f64 {
    // macOS: read the process RSS via `ps`. Shell wrapper pipes this in
    // via the RSS env var if set; otherwise use the getrusage shim.
    // For simplicity, print 0; the shell watchdog tracks RSS externally.
    0.0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n_hands: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(64);

    println!("phase5_measure: n_hands={}", n_hands);

    // River (5-card) build.
    let t0 = std::time::Instant::now();
    let s = build(vec![12, 24, 37, 50, 3], n_hands, 1);
    let dt = t0.elapsed();
    println!("  RIVER (5 cards): build={:?}  root_acts={}", dt, s.n_root_actions());
    drop(s);

    // Turn (4-card) build — the Phase-5 leaf shape (46 rivers).
    let t0 = std::time::Instant::now();
    let s = build(vec![12, 24, 37, 50], n_hands, 2);
    let dt = t0.elapsed();
    println!("  TURN  (4 cards): build={:?}  root_acts={}", dt, s.n_root_actions());
    let turn_build_ms = dt.as_secs_f64() * 1000.0;
    drop(s);

    // Do NOT build a flop tree. Extrapolate from the turn build: the
    // flop tree is 47 nested turn trees.
    let flop_build_ms_est = turn_build_ms * 47.0;
    println!();
    println!("  phase5 step-1 numbers:");
    println!("    turn build       = {:.1} ms", turn_build_ms);
    println!("    flop build (est) = {:.1} ms  (= turn x 47)", flop_build_ms_est);
    println!("    1.29M flops x flop build = {:.1} hours  (build only)",
             flop_build_ms_est * 1_290_000.0 / 1000.0 / 3600.0);
    println!();
    if flop_build_ms_est > 1.0 {
        println!("  VERDICT: flop leaf cost > 1 ms -> Phase 5 at this");
        println!("  resolution is infeasible without a coarsened leaf table.");
    } else {
        println!("  VERDICT: flop leaf cost <= 1 ms -> worth costing the table.");
    }
    let _ = rss_mb();
}
