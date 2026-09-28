use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_subgame::{run_poc, POCConfig, Range};

fn flop_root(b: &[u8; 3]) -> GameState {
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.advance_street_in_place(b);
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s
}

fn make_range(pool: &[u8], excl: &[u8], n: usize) -> Vec<[u8; 2]> {
    let avail: Vec<u8> = pool.iter().copied().filter(|c| !excl.contains(c)).collect();
    let mut hands = Vec::new();
    'outer: for i in 0..avail.len() {
        for j in (i+1)..avail.len() {
            hands.push([avail[i], avail[j]]);
            if hands.len() >= n { break 'outer; }
        }
    }
    hands
}

#[test]
#[ignore = "slow: builds full nested flop chance tree (>4 min); run with --ignored"]
fn flop_solver_builds_nested_chance_tree() {
    let board: [u8; 3] = [0, 14, 28];
    let root = flop_root(&board);
    assert_eq!(root.street, Street::Flop);

    // 3x3 hands = 9 deals. Flop tree memory scales as n_nodes * n_deals,
    // and n_nodes is ~200K for a full flop tree. 9 deals keeps peak RSS
    // under 500 MB. Production would need sparse regret storage.
    let p0_pool: Vec<u8> = (0u8..26).collect();
    let p1_pool: Vec<u8> = (26u8..52).collect();
    let p0_hands = make_range(&p0_pool, &board, 3);
    let p1_hands = make_range(&p1_pool, &board, 3);
    println!("p0_hands: {:?}", p0_hands);
    println!("p1_hands: {:?}", p1_hands);

    let evaluator = pkr_eval::NlheEvaluator;
    let cfg = POCConfig {
        root,
        p0_range: Range::uniform(p0_hands),
        p1_range: Range::uniform(p1_hands),
        iterations: 2,
        evaluator: &evaluator,
        blueprint: None,
    };

    let t0 = std::time::Instant::now();
    let r = run_poc(&cfg);
    let dt = t0.elapsed();

    println!();
    println!("=== FLOP SOLVER SANITY ===");
    println!("  board:         {:?}", board);
    println!("  iterations:    {}", r.iterations);
    println!("  nodes visited: {}", r.nodes_visited);
    println!("  wall:          {:.2}s", dt.as_secs_f64());
    println!("  BR_v1 vs CFR:  {:.4} chips", r.br_v1_vs_cfr);

    assert!(r.br_v1_vs_cfr.is_finite());
    assert!(r.nodes_visited > 100_000,
        "flop tree should be large, got {}", r.nodes_visited);
}
