use pkr_contracts::Evaluator;
use rand::rng;
use rand::seq::SliceRandom;

/// Number of Monte Carlo iterations for EHS calculation.
const NUM_SAMPLES: usize = 100; // reduced for precomputation; runtime uses precomputed table

/// Calculates Expected Hand Strength (EHS) and EHS² for a given situation.
pub fn calculate_ehs(hole: &[u8], board: &[u8], evaluator: &dyn Evaluator) -> (f32, f32) {
    assert_eq!(hole.len(), 2, "exactly 2 hole cards required");
    assert!(board.len() <= 5, "board cannot exceed 5 cards");
    let needed_board_cards = 5 - board.len();
    let total_cards_needed = 2 + needed_board_cards;

    let mut remaining: Vec<u8> = (0..52u8)
        .filter(|c| !hole.contains(c) && !board.contains(c))
        .collect();

    let mut rng = rng();
    let samples = NUM_SAMPLES;

    let mut sum_equity: f64 = 0.0;
    let mut sum_sq: f64 = 0.0;

    for _ in 0..samples {
        remaining.shuffle(&mut rng);
        let mut draw = remaining.iter().take(total_cards_needed);
        let opp_hole: Vec<u8> = draw.by_ref().take(2).cloned().collect();
        let board_completion: Vec<u8> = draw.take(needed_board_cards).cloned().collect();

        let mut full_board = board.to_vec();
        full_board.extend(board_completion);

        let hero_rank = evaluator.evaluate_hand(hole, &full_board);
        let opp_rank = evaluator.evaluate_hand(&opp_hole, &full_board);

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
