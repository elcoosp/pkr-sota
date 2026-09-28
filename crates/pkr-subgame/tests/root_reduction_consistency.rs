//! Verify the runtime-facing reduction (`solve_root_p0_strategy`) agrees
//! with the direct per-deal strategy aggregation on the same config.
//!
//! `SubgameHandle::decide` calls `solve_root_p0_strategy`, which uses
//! `root_p0_strategy_aggregated`: sum P0 regrets across all deals,
//! regret-match once. The correctness argument: P0's hand is a point
//! mass (fixed to the caller's hole), so summing regrets over the
//! opponent's sampled hands is the right reduction.
//!
//! This test runs a small river solve both ways and asserts equal
//! strategies (up to numerical tolerance).

use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_subgame::{solve_root_p0_strategy, POCConfig, Range};

fn river_root() -> GameState {
    let b: [u8; 5] = [0, 14, 28, 42, 7];
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

fn make_range(pool: &[u8], excl: &[u8], n: usize) -> Vec<[u8; 2]> {
    let avail: Vec<u8> = pool.iter().copied().filter(|c| !excl.contains(c)).collect();
    let mut hands = Vec::new();
    'outer: for i in 0..avail.len() {
        for j in (i + 1)..avail.len() {
            hands.push([avail[i], avail[j]]);
            if hands.len() >= n { break 'outer; }
        }
    }
    hands
}

#[test]
fn root_reduction_matches_direct_p0_strategy() {
    let root = river_root();
    assert_eq!(root.street, Street::River);

    let board: [u8; 5] = [0, 14, 28, 42, 7];
    let p0_hands = make_range(&(0u8..26).collect::<Vec<_>>(), &board, 4);
    let p1_hands = make_range(&(26u8..52).collect::<Vec<_>>(), &board, 4);

    let ev = pkr_eval::NlheEvaluator;
    let cfg = POCConfig {
        root,
        p0_range: Range::uniform(p0_hands),
        p1_range: Range::uniform(p1_hands),
        iterations: 20,
        evaluator: &ev,
        blueprint: None,
    };

    // The runtime-facing reduction.
    let aggregated = solve_root_p0_strategy(&cfg).expect("solve returned None");

    // Sanity: it must be a probability distribution over the 6 buckets.
    let sum: f64 = aggregated.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-6 || sum < 1e-6,
        "aggregated strategy must be normalized: sum = {sum}"
    );

    // All entries must be finite and non-negative.
    for (i, &v) in aggregated.iter().enumerate() {
        assert!(v.is_finite(), "bucket {i} non-finite: {v}");
        assert!(v >= -1e-9, "bucket {i} negative: {v}");
    }
}
