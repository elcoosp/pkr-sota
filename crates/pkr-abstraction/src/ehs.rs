use pkr_contracts::Evaluator;
use rand::rng;
use rand::seq::SliceRandom;
use std::sync::OnceLock;

/// Returns the configured EHS sample count, cached after first read.
fn num_samples() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("EHS_SAMPLES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000)
    })
}

/// Calculates Expected Hand Strength (EHS) and EHS².
/// Uses stack arrays and partial shuffling to avoid allocations.
pub fn calculate_ehs(hole: &[u8], board: &[u8], evaluator: &dyn Evaluator) -> (f32, f32) {
    assert_eq!(hole.len(), 2, "exactly 2 hole cards required");
    assert!(board.len() <= 5, "board cannot exceed 5 cards");
    let needed_board = 5 - board.len();
    let total_draw = 2 + needed_board; // 2 opponent cards + board completion

    let samples = num_samples();

    // Working arrays on the stack (max 7 cards needed)
    let mut remaining: Vec<u8> = (0..52u8)
        .filter(|c| !hole.contains(c) && !board.contains(c))
        .collect();

    let mut rng = rng();
    let mut sum_equity: f64 = 0.0;
    let mut sum_sq: f64 = 0.0;

    // Pre-fill full_board with known board cards
    let mut full_board_buf = [0u8; 5];
    full_board_buf[..board.len()].copy_from_slice(board);

    for _ in 0..samples {
        // Partial shuffle: only shuffle the first `total_draw` elements
        remaining.partial_shuffle(&mut rng, total_draw);

        let opp_hole = [remaining[0], remaining[1]];
        let board_fill = &remaining[2..2 + needed_board];
        full_board_buf[board.len()..].copy_from_slice(board_fill);

        let hero_rank = evaluator.evaluate_hand(hole, &full_board_buf);
        let opp_rank = evaluator.evaluate_hand(&opp_hole, &full_board_buf);

        let equity = if hero_rank < opp_rank {
            1.0
        } else if hero_rank == opp_rank {
            0.5
        } else {
            0.0
        };

        sum_equity += equity;
        sum_sq += equity * equity;
    }

    let ehs = (sum_equity / samples as f64) as f32;
    let ehs_sq = (sum_sq / samples as f64) as f32;
    (ehs, ehs_sq)
}
