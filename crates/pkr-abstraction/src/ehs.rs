use pkr_contracts::Evaluator;
use rand::rng;
use rand::seq::SliceRandom;

/// Number of Monte Carlo iterations for EHS calculation.
const NUM_SAMPLES: usize = 1000;

/// Calculates Expected Hand Strength (EHS) and EHS² for a given situation.
///
/// * `hole` – the player's hole cards (2 cards).
/// * `board` – community cards (0, 3, 4 or 5 cards for pre‑flop, flop, turn, river).
/// * `evaluator` – hand evaluator (e.g. `NlheEvaluator`).
///
/// Returns `(ehs, ehs_squared)` where both are in `[0.0, 1.0]`.
pub fn calculate_ehs(hole: &[u8], board: &[u8], evaluator: &dyn Evaluator) -> (f32, f32) {
    assert_eq!(hole.len(), 2, "exactly 2 hole cards required");
    assert!(board.len() <= 5, "board cannot exceed 5 cards");
    let needed_board_cards = 5 - board.len();
    let total_cards_needed = 2 + needed_board_cards;

    // Build deck (0..52) and remove known cards
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

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_eval::NlheEvaluator;

    /// Convert (suit 0‑3, rank 2‑14) to the internal card index (0‑51).
    fn card_idx(suit: u8, rank: u8) -> u8 {
        assert!(suit < 4, "suit must be 0-3");
        assert!((2..=14).contains(&rank), "rank must be 2-14");
        (rank - 2) + suit * 13
    }

    #[test]
    fn test_pocket_aces_preflop_high_equity() {
        let evaluator = NlheEvaluator;
        // A♠ (suit 0, rank 14) and A♥ (suit 1, rank 14)
        let hole = vec![card_idx(0, 14), card_idx(1, 14)];
        let board = vec![];

        let (ehs, ehs_sq) = calculate_ehs(&hole, &board, &evaluator);

        // Pocket aces have ~85% equity preflop against a random hand.
        assert!(
            ehs > 0.8,
            "EHS for AA preflop should be >0.8, got {:.4}",
            ehs
        );
        assert!(ehs <= 1.0, "EHS must be ≤ 1.0");
        // EHS² ≥ EHS² (definition of second moment)
        assert!(ehs_sq >= ehs * ehs, "EHS² must be ≥ EHS²");
        // Variance = EHS² - EHS² > 0 because the outcome is not deterministic
        let variance = ehs_sq - ehs * ehs;
        assert!(
            variance > 0.0,
            "Variance must be positive, got {:.6}",
            variance
        );
    }

    #[test]
    fn test_nuts_on_river_equity_one() {
        let evaluator = NlheEvaluator;
        // Board: Ah Ad As Kc Qd
        // A♥=25, A♦=38, A♠=12, K♣=50, Q♦=36
        // No straight flush possible.
        let board = vec![25, 38, 12, 50, 36];

        // Hero holds the last ace (A♣ = suit3 rank14 = idx 51) + any other card
        let hole = vec![51, 0]; // A♣ + 2♠

        let (ehs, ehs_sq) = calculate_ehs(&hole, &board, &evaluator);

        // Hero has quad aces, which is the nuts on this board → equity = 1.0
        assert!(
            (ehs - 1.0).abs() < 0.01,
            "Quad aces on river should have EHS ~1.0, got {:.4}",
            ehs
        );
        assert!(
            (ehs_sq - 1.0).abs() < 0.01,
            "EHS² should be ~1.0, got {:.4}",
            ehs_sq
        );
    }
}
