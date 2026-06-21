use pkr_contracts::Evaluator;

pub struct NlheEvaluator;

impl Evaluator for NlheEvaluator {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u16 {
        // Stub: returns 0 to make tests fail
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: map suit (0-3) and rank (2-14, Ace=14) to card index 0-51
    // rank: 2=0,3=1,...14=12
    fn card_idx(suit: u8, rank: u8) -> u8 {
        assert!(suit < 4);
        assert!(rank >= 2 && rank <= 14);
        let rank_idx = rank - 2;
        suit * 13 + rank_idx
    }

    // Royal flush: A♠ K♠ Q♠ J♠ 10♠ + two junk
    #[test]
    fn royal_flush_vs_four_of_a_kind() {
        let evaluator = NlheEvaluator;
        // Spades: Ace=14->idx 12, King=13->11, Queen=12->10, Jack=11->9, Ten=10->8
        let hole = vec![card_idx(0, 14), card_idx(0, 13)]; // A♠ K♠
        let board = vec![card_idx(0, 12), card_idx(0, 11), card_idx(0, 10)]; // Q♠ J♠ 10♠
        let royal_flush_score = evaluator.evaluate_hand(&hole, &board);

        // Four Aces (all suits) + a King
        let hole4 = vec![card_idx(0, 14), card_idx(1, 14)]; // A♠ A♥
        let board4 = vec![card_idx(2, 14), card_idx(3, 14), card_idx(0, 13)]; // A♦ A♣ K♠
        let four_kind_score = evaluator.evaluate_hand(&hole4, &board4);

        assert!(
            royal_flush_score < four_kind_score,
            "Royal flush (score {}) should be better (lower) than four of a kind (score {})",
            royal_flush_score,
            four_kind_score
        );
    }

    #[test]
    fn full_house_vs_flush() {
        let evaluator = NlheEvaluator;
        // Full house: three 8's and two 5's
        let hole = vec![card_idx(0, 8), card_idx(1, 8)];
        let board = vec![card_idx(2, 8), card_idx(0, 5), card_idx(1, 5)];
        let fh_score = evaluator.evaluate_hand(&hole, &board);

        // Flush: all hearts (suit 1), not a straight
        let hole_f = vec![card_idx(1, 14), card_idx(1, 3)]; // A♥ 3♥
        let board_f = vec![card_idx(1, 5), card_idx(1, 7), card_idx(1, 9)]; // 5♥ 7♥ 9♥
        let flush_score = evaluator.evaluate_hand(&hole_f, &board_f);

        assert!(
            fh_score < flush_score,
            "Full house (score {}) should beat flush (score {})",
            fh_score,
            flush_score
        );
    }

    #[test]
    fn high_card_ordering() {
        let evaluator = NlheEvaluator;
        // High card hand with Ace high
        let hole1 = vec![card_idx(0, 14), card_idx(1, 3)];
        let board1 = vec![card_idx(2, 5), card_idx(3, 7), card_idx(0, 9)];
        let score1 = evaluator.evaluate_hand(&hole1, &board1);

        // High card hand with King high (no Ace)
        let hole2 = vec![card_idx(0, 13), card_idx(1, 3)];
        let board2 = vec![card_idx(2, 5), card_idx(3, 7), card_idx(0, 9)];
        let score2 = evaluator.evaluate_hand(&hole2, &board2);

        assert!(
            score1 < score2,
            "Ace-high (score {}) should be better (lower) than King-high (score {})",
            score1,
            score2
        );
    }

    #[test]
    fn straight_vs_three_of_a_kind() {
        let evaluator = NlheEvaluator;
        // Straight: 9-8-7-6-5 mixed suits
        let hole = vec![card_idx(0, 9), card_idx(1, 8)];
        let board = vec![card_idx(2, 7), card_idx(3, 6), card_idx(0, 5)];
        let straight_score = evaluator.evaluate_hand(&hole, &board);

        // Three of a kind: three 4's
        let hole3 = vec![card_idx(0, 4), card_idx(1, 4)];
        let board3 = vec![card_idx(2, 4), card_idx(0, 9), card_idx(1, 10)];
        let trips_score = evaluator.evaluate_hand(&hole3, &board3);

        assert!(
            straight_score < trips_score,
            "Straight (score {}) should beat trips (score {})",
            straight_score,
            trips_score
        );
    }
}
