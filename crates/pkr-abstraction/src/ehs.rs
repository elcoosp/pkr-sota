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

    // --- Additional tests for deeper coverage ---

    #[test]
    fn test_weak_hand_preflop_low_equity() {
        let evaluator = NlheEvaluator;
        // 7♠ 2♦ (off-suit, worst hand)
        let hole = vec![card_idx(0, 7), card_idx(2, 2)];
        let board = vec![];
        let (ehs, ehs_sq) = calculate_ehs(&hole, &board, &evaluator);
        assert!(
            ehs < 0.45,
            "72o preflop should have equity <0.45, got {:.4}",
            ehs
        );
        assert!(ehs >= 0.0);
        assert!(ehs_sq >= ehs * ehs);
    }

    #[test]
    fn test_flop_equity_with_flush_draw() {
        let evaluator = NlheEvaluator;
        // Hero: A♠ K♠ (spades)
        let hole = vec![card_idx(0, 14), card_idx(0, 13)];
        // Flop: Q♠ 5♠ 2♥  (two spades → hero has nut flush draw)
        let board = vec![
            card_idx(0, 12), // Q♠
            card_idx(0, 5),  // 5♠
            card_idx(1, 2),  // 2♥
        ];
        let (ehs, ehs_sq) = calculate_ehs(&hole, &board, &evaluator);
        // Nut flush draw + two overcards should have > 0.45 equity
        assert!(
            ehs > 0.4,
            "Nut flush draw on flop should have equity >0.4, got {:.4}",
            ehs
        );
        assert!(ehs <= 1.0);
        assert!(ehs_sq >= ehs * ehs);
    }

    #[test]
    fn test_turn_equity_variance_smaller_than_flop() {
        let evaluator = NlheEvaluator;
        // Hero: A♠ A♥
        let hole = vec![card_idx(0, 14), card_idx(1, 14)];
        // Flop: K♠ 7♦ 2♣ (dry flop)
        let flop = vec![card_idx(0, 13), card_idx(2, 7), card_idx(3, 2)];
        let (ehs_flop, ehs_sq_flop) = calculate_ehs(&hole, &flop, &evaluator);
        let var_flop = ehs_sq_flop - ehs_flop * ehs_flop;

        // Turn adds Q♥
        let turn = vec![
            card_idx(0, 13),
            card_idx(2, 7),
            card_idx(3, 2),
            card_idx(1, 12),
        ];
        let (ehs_turn, ehs_sq_turn) = calculate_ehs(&hole, &turn, &evaluator);
        let var_turn = ehs_sq_turn - ehs_turn * ehs_turn;

        assert!(
            var_turn < var_flop + 0.02, // allow some MC noise
            "Variance on turn ({:.5}) should be <= variance on flop ({:.5})",
            var_turn,
            var_flop
        );
    }

    #[test]
    fn test_equity_against_multiple_opponents_not_supported() {
        // This test ensures the function signature is respected (only 1 opponent)
        let evaluator = NlheEvaluator;
        let hole = vec![0, 1];
        let board = vec![2, 3, 4];
        // Just verify it doesn't panic for a valid call
        let (ehs, _) = calculate_ehs(&hole, &board, &evaluator);
        assert!((0.0..=1.0).contains(&ehs));
    }

    #[test]
    fn test_ehs_squared_approximately_equals_variance_plus_ehs_squared() {
        let evaluator = NlheEvaluator;
        let hole = vec![card_idx(0, 10), card_idx(1, 10)]; // JJ
        let board = vec![];
        let (ehs, ehs_sq) = calculate_ehs(&hole, &board, &evaluator);
        let variance = ehs_sq - ehs * ehs;
        assert!(
            variance >= 0.0,
            "Variance must be non-negative, got {:.6}",
            variance
        );
        // For a non-deterministic outcome, variance should be > 0
        assert!(variance > 0.0, "Preflop JJ should have positive variance");
    }

    #[test]
    fn test_known_board_no_community_cards_needed() {
        // When board is already 5 cards, no board completion needed.
        let evaluator = NlheEvaluator;
        // Royal flush in spades as board
        let board = vec![
            card_idx(0, 10), // 10♠
            card_idx(0, 11), // J♠
            card_idx(0, 12), // Q♠
            card_idx(0, 13), // K♠
            card_idx(0, 14), // A♠
        ];
        // Any hole cards (they don't play)
        let hole = vec![card_idx(1, 2), card_idx(2, 2)]; // 2♥ 2♦
        let (ehs, ehs_sq) = calculate_ehs(&hole, &board, &evaluator);
        // All players share the board → tie always → equity = 0.5
        assert!(
            (ehs - 0.5).abs() < 0.01,
            "Shared royal flush should give EHS 0.5, got {:.4}",
            ehs
        );
        assert!(
            (ehs_sq - 0.25).abs() < 0.01,
            "EHS² should be 0.25, got {:.4}",
            ehs_sq
        );
    }
}
