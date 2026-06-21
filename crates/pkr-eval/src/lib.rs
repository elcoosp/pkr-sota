use pkr_contracts::Evaluator;

mod tables;

/// A hand evaluator for No-Limit Hold'em using a lazy precomputed lookup table.
///
/// Expects exactly 2 hole cards and 5 board cards (total 7).
pub struct NlheEvaluator;

impl Evaluator for NlheEvaluator {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u16 {
        assert_eq!(hole.len(), 2, "NlheEvaluator expects exactly 2 hole cards");
        assert_eq!(
            board.len(),
            5,
            "NlheEvaluator expects exactly 5 board cards"
        );

        let mut cards = [0u8; 7];
        cards[..2].copy_from_slice(hole);
        cards[2..].copy_from_slice(board);
        cards.sort_unstable();

        let mut best = u16::MAX;
        for skip1 in 0..7 {
            for skip2 in (skip1 + 1)..7 {
                let mut hand = [0u8; 5];
                let mut idx = 0;
                for (k, &card) in cards.iter().enumerate() {
                    if k != skip1 && k != skip2 {
                        hand[idx] = card;
                        idx += 1;
                    }
                }
                let rank = tables::five_card_rank(hand);
                if rank < best {
                    best = rank;
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card_idx(suit: u8, rank: u8) -> u8 {
        assert!(suit < 4);
        assert!(rank >= 2 && rank <= 14);
        (rank - 2) + suit * 13
    }

    fn eval(hole: &[u8], board: &[u8]) -> u16 {
        NlheEvaluator.evaluate_hand(hole, board)
    }

    // --- Hand category comparisons ---

    #[test]
    fn royal_flush_vs_four_of_a_kind() {
        let hole = vec![card_idx(0, 14), card_idx(0, 13)];
        let board = vec![
            card_idx(0, 12),
            card_idx(0, 11),
            card_idx(0, 10),
            card_idx(2, 2),
            card_idx(3, 3),
        ];
        let rf = eval(&hole, &board);

        let hole4 = vec![card_idx(0, 14), card_idx(1, 14)];
        let board4 = vec![
            card_idx(2, 14),
            card_idx(3, 14),
            card_idx(0, 13),
            card_idx(1, 3),
            card_idx(2, 4),
        ];
        let fk = eval(&hole4, &board4);

        assert!(
            rf < fk,
            "Royal flush ({}) should be lower than four of a kind ({})",
            rf,
            fk
        );
    }

    #[test]
    fn full_house_vs_flush() {
        let hole = vec![card_idx(0, 8), card_idx(1, 8)];
        let board = vec![
            card_idx(2, 8),
            card_idx(0, 5),
            card_idx(1, 5),
            card_idx(3, 2),
            card_idx(3, 4),
        ];
        let fh = eval(&hole, &board);

        let hole_f = vec![card_idx(1, 14), card_idx(1, 3)];
        let board_f = vec![
            card_idx(1, 5),
            card_idx(1, 7),
            card_idx(1, 9),
            card_idx(0, 2),
            card_idx(2, 4),
        ];
        let fl = eval(&hole_f, &board_f);

        assert!(fh < fl, "Full house ({}) should beat flush ({})", fh, fl);
    }

    #[test]
    fn straight_vs_three_of_a_kind() {
        let hole = vec![card_idx(0, 9), card_idx(1, 8)];
        let board = vec![
            card_idx(2, 7),
            card_idx(3, 6),
            card_idx(0, 5),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let st = eval(&hole, &board);

        let hole3 = vec![card_idx(0, 4), card_idx(1, 4)];
        let board3 = vec![
            card_idx(2, 4),
            card_idx(0, 9),
            card_idx(1, 10),
            card_idx(2, 11),
            card_idx(3, 12),
        ];
        let trips = eval(&hole3, &board3);

        assert!(
            st < trips,
            "Straight ({}) should beat trips ({})",
            st,
            trips
        );
    }

    #[test]
    fn high_card_ordering() {
        let hole1 = vec![card_idx(0, 14), card_idx(1, 3)];
        let board1 = vec![
            card_idx(2, 5),
            card_idx(3, 7),
            card_idx(0, 9),
            card_idx(1, 10),
            card_idx(2, 12),
        ];
        let ace = eval(&hole1, &board1);

        let hole2 = vec![card_idx(0, 13), card_idx(1, 3)];
        let board2 = board1.clone();
        let king = eval(&hole2, &board2);

        assert!(
            ace < king,
            "Ace-high ({}) should beat King-high ({})",
            ace,
            king
        );
    }

    #[test]
    fn implements_evaluator_trait() {
        fn assert_evaluator<T: Evaluator>() {}
        assert_evaluator::<NlheEvaluator>();
    }

    // --- Additional tests for deeper coverage ---

    #[test]
    fn two_pair_vs_one_pair() {
        let hole2p = vec![card_idx(0, 8), card_idx(1, 8)];
        let board2p = vec![
            card_idx(2, 5),
            card_idx(3, 5),
            card_idx(0, 2),
            card_idx(1, 9),
            card_idx(2, 11),
        ];
        let two_pair = eval(&hole2p, &board2p);

        let hole1p = vec![card_idx(0, 14), card_idx(1, 8)];
        let board1p = vec![
            card_idx(2, 5),
            card_idx(3, 7),
            card_idx(0, 9),
            card_idx(1, 10),
            card_idx(2, 12),
        ];
        let one_pair = eval(&hole1p, &board1p);

        assert!(
            two_pair < one_pair,
            "Two pair ({}) should beat one pair ({})",
            two_pair,
            one_pair
        );
    }

    #[test]
    fn two_pair_kicker_breaks_tie() {
        let hole1 = vec![card_idx(0, 8), card_idx(1, 5)];
        let board1 = vec![
            card_idx(2, 8),
            card_idx(3, 5),
            card_idx(0, 14), // Ace kicker
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let higher_kicker = eval(&hole1, &board1);

        let hole2 = vec![card_idx(0, 8), card_idx(1, 5)];
        let board2 = vec![
            card_idx(2, 8),
            card_idx(3, 5),
            card_idx(0, 13), // King kicker
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let lower_kicker = eval(&hole2, &board2);

        assert!(
            higher_kicker < lower_kicker,
            "Two pair with Ace kicker ({}) should be better than King kicker ({})",
            higher_kicker,
            lower_kicker
        );
    }

    #[test]
    fn equal_hands_same_rank() {
        let hole1 = vec![card_idx(0, 10), card_idx(0, 9)];
        let board1 = vec![
            card_idx(0, 8),
            card_idx(0, 7),
            card_idx(0, 6),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let sf1 = eval(&hole1, &board1);

        let hole2 = vec![card_idx(1, 10), card_idx(1, 9)];
        let board2 = vec![
            card_idx(1, 8),
            card_idx(1, 7),
            card_idx(1, 6),
            card_idx(0, 2),
            card_idx(3, 3),
        ];
        let sf2 = eval(&hole2, &board2);

        assert_eq!(sf1, sf2, "Identical hands in different suits must tie");
    }

    #[test]
    fn wheel_vs_six_high_straight() {
        let hole = vec![card_idx(0, 14), card_idx(1, 2)];
        let board = vec![
            card_idx(2, 3),
            card_idx(3, 4),
            card_idx(0, 5),
            card_idx(1, 7),
            card_idx(2, 9),
        ];
        let wheel = eval(&hole, &board);

        let hole6 = vec![card_idx(0, 6), card_idx(1, 2)];
        let board6 = vec![
            card_idx(2, 3),
            card_idx(3, 4),
            card_idx(0, 5),
            card_idx(1, 7),
            card_idx(2, 8),
        ];
        let six_high = eval(&hole6, &board6);

        assert!(
            six_high < wheel,
            "6-high straight ({}) should beat wheel ({}), higher straight wins",
            six_high,
            wheel
        );
    }

    #[test]
    fn flush_beats_straight() {
        let hole = vec![card_idx(0, 14), card_idx(0, 3)];
        let board = vec![
            card_idx(0, 5),
            card_idx(0, 7),
            card_idx(0, 9),
            card_idx(1, 2),
            card_idx(2, 4),
        ];
        let flush = eval(&hole, &board);

        let hole_st = vec![card_idx(0, 9), card_idx(1, 8)];
        let board_st = vec![
            card_idx(2, 7),
            card_idx(3, 6),
            card_idx(0, 5),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let straight = eval(&hole_st, &board_st);

        assert!(
            flush < straight,
            "Flush ({}) should beat straight ({})",
            flush,
            straight
        );
    }

    #[test]
    fn royal_flush_is_best() {
        let hole = vec![card_idx(0, 14), card_idx(0, 13)];
        let board = vec![
            card_idx(0, 12),
            card_idx(0, 11),
            card_idx(0, 10),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let rank = eval(&hole, &board);
        assert_eq!(rank, 0, "Royal flush must be rank 0, got {}", rank);
    }

    #[test]
    fn four_of_a_kind_beats_full_house() {
        let hole4 = vec![card_idx(0, 7), card_idx(1, 7)];
        let board4 = vec![
            card_idx(2, 7),
            card_idx(3, 7),
            card_idx(0, 13),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let quads = eval(&hole4, &board4);

        let hole_fh = vec![card_idx(0, 14), card_idx(1, 14)];
        let board_fh = vec![
            card_idx(2, 14),
            card_idx(3, 5),
            card_idx(0, 5),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let fh = eval(&hole_fh, &board_fh);

        assert!(
            quads < fh,
            "Four of a kind ({}) should beat full house ({})",
            quads,
            fh
        );
    }

    #[test]
    fn three_of_a_kind_beats_two_pair() {
        let hole3 = vec![card_idx(0, 9), card_idx(1, 9)];
        let board3 = vec![
            card_idx(2, 9),
            card_idx(3, 4),
            card_idx(0, 5),
            card_idx(1, 8),
            card_idx(2, 11),
        ];
        let trips = eval(&hole3, &board3);

        let hole2p = vec![card_idx(0, 8), card_idx(1, 8)];
        let board2p = vec![
            card_idx(2, 5),
            card_idx(3, 5),
            card_idx(0, 2),
            card_idx(1, 9),
            card_idx(2, 11),
        ];
        let two_pair = eval(&hole2p, &board2p);

        assert!(
            trips < two_pair,
            "Three of a kind ({}) should beat two pair ({})",
            trips,
            two_pair
        );
    }

    #[test]
    fn one_pair_beats_high_card() {
        let hole1p = vec![card_idx(0, 7), card_idx(1, 7)];
        let board1p = vec![
            card_idx(2, 3),
            card_idx(3, 5),
            card_idx(0, 9),
            card_idx(1, 11),
            card_idx(2, 13),
        ];
        let pair = eval(&hole1p, &board1p);

        let hole_hc = vec![card_idx(0, 14), card_idx(1, 3)];
        let board_hc = vec![
            card_idx(2, 5),
            card_idx(3, 7),
            card_idx(0, 9),
            card_idx(1, 10),
            card_idx(2, 12),
        ];
        let high_card = eval(&hole_hc, &board_hc);

        assert!(
            pair < high_card,
            "One pair ({}) should beat high card ({})",
            pair,
            high_card
        );
    }

    #[test]
    fn ace_high_straight_flush_vs_king_high() {
        let hole_royal = vec![card_idx(0, 14), card_idx(0, 13)];
        let board_royal = vec![
            card_idx(0, 12),
            card_idx(0, 11),
            card_idx(0, 10),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let royal = eval(&hole_royal, &board_royal);

        let hole_king = vec![card_idx(0, 13), card_idx(0, 12)];
        let board_king = vec![
            card_idx(0, 11),
            card_idx(0, 10),
            card_idx(0, 9),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let king_high = eval(&hole_king, &board_king);

        assert!(
            royal < king_high,
            "Royal flush ({}) should beat King-high straight flush ({})",
            royal,
            king_high
        );
    }

    #[test]
    fn full_house_tiebreaker_by_trips() {
        let hole9 = vec![card_idx(0, 9), card_idx(1, 9)];
        let board9 = vec![
            card_idx(2, 9),
            card_idx(3, 5),
            card_idx(0, 5),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let nines_full = eval(&hole9, &board9);

        let hole8 = vec![card_idx(0, 8), card_idx(1, 8)];
        let board8 = vec![
            card_idx(2, 8),
            card_idx(3, 14),
            card_idx(0, 14),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let eights_full = eval(&hole8, &board8);

        assert!(
            nines_full < eights_full,
            "9s full ({}) should beat 8s full ({})",
            nines_full,
            eights_full
        );
    }

    #[test]
    fn one_pair_tiebreaker_by_kickers() {
        let hole1 = vec![card_idx(0, 14), card_idx(1, 14)];
        let board1 = vec![
            card_idx(2, 13),
            card_idx(3, 12),
            card_idx(0, 11),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let higher = eval(&hole1, &board1);

        let hole2 = vec![card_idx(0, 14), card_idx(1, 14)];
        let board2 = vec![
            card_idx(2, 13),
            card_idx(3, 12),
            card_idx(0, 10),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let lower = eval(&hole2, &board2);

        assert!(
            higher < lower,
            "Aces with K Q J ({}) should beat Aces with K Q T ({})",
            higher,
            lower
        );
    }

    #[test]
    fn high_card_tiebreaker_by_fifth_card() {
        let hole1 = vec![card_idx(0, 14), card_idx(1, 13)];
        let board1 = vec![
            card_idx(2, 12),
            card_idx(3, 11),
            card_idx(0, 9),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let nine = eval(&hole1, &board1);

        let hole2 = vec![card_idx(0, 14), card_idx(1, 13)];
        let board2 = vec![
            card_idx(2, 12),
            card_idx(3, 11),
            card_idx(0, 8),
            card_idx(1, 2),
            card_idx(2, 3),
        ];
        let eight = eval(&hole2, &board2);

        assert!(
            nine < eight,
            "A-K-Q-J-9 ({}) should beat A-K-Q-J-8 ({})",
            nine,
            eight
        );
    }

    #[test]
    fn evaluate_hand_requires_correct_lengths() {
        let evaluator = NlheEvaluator;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            evaluator.evaluate_hand(&[], &[0, 1, 2, 3, 4]);
        }));
        assert!(result.is_err(), "should panic on empty hole");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            evaluator.evaluate_hand(&[0, 1], &[0, 1, 2, 3, 4, 5]);
        }));
        assert!(result.is_err(), "should panic on 6 board cards");
    }
}
