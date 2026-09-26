//! POC: river CFR subgame solve.
//!
//! Single key test: does CFR-solved P0 reduce P1's best-response value
//! compared to a uniform-random P0? If yes, CFR is producing a real
//! strategy. If no, the solver has a bug.

use pkr_core::state::{Action, ActionKind, GameState};
use pkr_subgame::{run_poc, POCConfig, Range};

fn board() -> [u8; 5] {
    // Ranks 0,1,2,3,8 across suits 0,1,2,3,0.
    // Max 2 cards of any suit -> no flush possible for either player.
    // No pair, no straight, mixed high/low.
    [0, 14, 28, 42, 8]
}

fn make_range(pool: &[u8], exclude: &[u8], n: usize) -> Vec<[u8; 2]> {
    let available: Vec<u8> = pool
        .iter()
        .copied()
        .filter(|c| !exclude.contains(c))
        .collect();
    let mut hands = Vec::new();
    'outer: for i in 0..available.len() {
        for j in (i + 1)..available.len() {
            hands.push([available[i], available[j]]);
            if hands.len() >= n {
                break 'outer;
            }
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

fn iters() -> u32 {
    std::env::var("PKR_POC_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50)
}

fn hand_count() -> usize {
    std::env::var("PKR_POC_HANDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12)
}

#[test]
fn poc_river_cfr_beats_uniform() {
    let b = board();
    // Disjoint pools, spread over both suits in each side.
    // P0: cards 0..26 (suits 0, 1). Board removes 0 and 8 (s0) and 14 (s1).
    // P1: cards 26..52 (suits 2, 3). Board removes 28 (s2) and 42 (s3).
    let p0_pool: Vec<u8> = (0u8..26).collect();
    let p1_pool: Vec<u8> = (26u8..52).collect();
    let n = hand_count();

    let p0_hands = make_range(&p0_pool, &b, n);
    let p1_hands = make_range(&p1_pool, &b, n);
    println!("board       = {:?}", b);
    println!("P0 range    = {} hands", p0_hands.len());
    println!("P1 range    = {} hands", p1_hands.len());

    let evaluator = pkr_eval::NlheEvaluator;

    // Run 1: CFR-solved P0
    let cfg_cfr = POCConfig {
        root: river_root(&b),
        p0_range: Range::uniform(p0_hands.clone()),
        p1_range: Range::uniform(p1_hands.clone()),
        iterations: iters(),
        evaluator: &evaluator,
        blueprint: None,
    };
    let t0 = std::time::Instant::now();
    let r_cfr = run_poc(&cfg_cfr);
    let dt_cfr = t0.elapsed();
    println!();
    println!("=== CFR-solved P0 ===");
    println!("  iterations:      {}", r_cfr.iterations);
    println!("  nodes visited:   {}", r_cfr.nodes_visited);
    println!("  wall time:       {:.2}s", dt_cfr.as_secs_f64());
    println!("  BR_v1 vs CFR:    {:.4} chips", r_cfr.br_v1_vs_cfr);

    // Run 2: uniform P0 (baseline: no learning)
    let cfg_uniform = POCConfig {
        root: river_root(&b),
        p0_range: Range::uniform(p0_hands),
        p1_range: Range::uniform(p1_hands),
        iterations: 0,  // no CFR iterations -> P0 strategy is not used, uniform is used in BR
        evaluator: &evaluator,
        blueprint: None,
    };
    let t1 = std::time::Instant::now();
    let r_uniform = run_poc(&cfg_uniform);
    let dt_uniform = t1.elapsed();
    println!();
    println!("=== Uniform P0 (baseline) ===");
    println!("  wall time:       {:.2}s", dt_uniform.as_secs_f64());
    println!("  BR_v1 vs uniform:{:.4} chips", r_uniform.br_v1_vs_cfr);

    // With 0 iterations, the CFR strategy map is empty; run_poc's
    // compute_br_v1 falls back to uniform_over_legal. That IS the
    // uniform baseline we want.
    let improvement = r_uniform.br_v1_vs_cfr - r_cfr.br_v1_vs_cfr;
    let pct = if r_uniform.br_v1_vs_cfr.abs() > 1e-9 {
        100.0 * improvement / r_uniform.br_v1_vs_cfr.abs()
    } else {
        0.0
    };

    println!();
    println!("=== Result ===");
    println!("  uniform BR_v1:   {:.4} chips", r_uniform.br_v1_vs_cfr);
    println!("  CFR     BR_v1:   {:.4} chips", r_cfr.br_v1_vs_cfr);
    println!("  improvement:     {:.4} chips ({:+.1}%)", improvement, pct);

    // CFR should reduce P1's BR value (make P0 harder to exploit).
    assert!(
        r_cfr.br_v1_vs_cfr < r_uniform.br_v1_vs_cfr,
        "CFR should reduce BR value: got CFR={} vs uniform={}",
        r_cfr.br_v1_vs_cfr, r_uniform.br_v1_vs_cfr,
    );
}
