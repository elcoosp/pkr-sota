pub mod lookup;
pub mod slow;
pub use slow::NlheEvaluator;
pub use lookup::TableEvaluator;

#[cfg(test)]
mod tests {
    use super::slow::NlheEvaluator;
    use pkr_contracts::Evaluator;

    fn card_idx(suit: u8, rank: u8) -> u8 {
        assert!(suit < 4);
        assert!(rank >= 2 && rank <= 14);
        (rank - 2) + suit * 13
    }

    fn eval(hole: &[u8], board: &[u8]) -> u32 {
        NlheEvaluator.evaluate_hand(hole, board)
    }

    #[test]
    fn royal_flush_vs_four_of_a_kind() {
        let hole = vec![card_idx(0,14), card_idx(0,13)];
        let board = vec![card_idx(0,12), card_idx(0,11), card_idx(0,10), card_idx(2,2), card_idx(3,3)];
        let rf = eval(&hole, &board);
        let hole4 = vec![card_idx(0,14), card_idx(1,14)];
        let board4 = vec![card_idx(2,14), card_idx(3,14), card_idx(0,13), card_idx(1,3), card_idx(2,4)];
        let fk = eval(&hole4, &board4);
        assert!(rf < fk);
    }

    #[test]
    fn full_house_vs_flush() {
        let hole = vec![card_idx(0,8), card_idx(1,8)];
        let board = vec![card_idx(2,8), card_idx(0,5), card_idx(1,5), card_idx(3,2), card_idx(3,4)];
        let fh = eval(&hole, &board);
        let hole_f = vec![card_idx(1,14), card_idx(1,3)];
        let board_f = vec![card_idx(1,5), card_idx(1,7), card_idx(1,9), card_idx(0,2), card_idx(2,4)];
        let fl = eval(&hole_f, &board_f);
        assert!(fh < fl);
    }

    #[test]
    fn straight_vs_three_of_a_kind() {
        let hole = vec![card_idx(0,9), card_idx(1,8)];
        let board = vec![card_idx(2,7), card_idx(3,6), card_idx(0,5), card_idx(1,2), card_idx(2,3)];
        let st = eval(&hole, &board);
        let hole3 = vec![card_idx(0,4), card_idx(1,4)];
        let board3 = vec![card_idx(2,4), card_idx(0,9), card_idx(1,10), card_idx(2,11), card_idx(3,12)];
        let trips = eval(&hole3, &board3);
        assert!(st < trips);
    }

    #[test]
    fn high_card_ordering() {
        let hole1 = vec![card_idx(0,14), card_idx(1,3)];
        let board1 = vec![card_idx(2,5), card_idx(3,7), card_idx(0,9), card_idx(1,10), card_idx(2,12)];
        let ace = eval(&hole1, &board1);
        let hole2 = vec![card_idx(0,13), card_idx(1,3)];
        let board2 = board1.clone();
        let king = eval(&hole2, &board2);
        assert!(ace < king);
    }

    #[test]
    fn implements_evaluator_trait() {
        fn assert_evaluator<T: Evaluator>() {}
        assert_evaluator::<NlheEvaluator>();
    }
}
#[cfg(test)]
mod extended_tests {
    use crate::slow::NlheEvaluator;
    use pkr_contracts::Evaluator;

    fn card(suit: u8, rank: u8) -> u8 { (rank - 2) + suit * 13 }

    #[test]
    fn test_two_pair_vs_one_pair() {
        // Board: K K 2 5 8 – no straight possible, two pair AAKK vs one pair KKA
        let tp = NlheEvaluator.evaluate_hand(&[card(0,14), card(1,14)], &[card(2,13), card(3,13), card(0,2), card(1,5), card(2,8)]);
        let op = NlheEvaluator.evaluate_hand(&[card(0,14), card(1,6)], &[card(2,13), card(3,13), card(0,2), card(1,5), card(2,8)]);
        assert!(tp < op, "Two pair should beat one pair");
    }

    #[test]
    fn test_flush_vs_straight() {
        let fl = NlheEvaluator.evaluate_hand(&[card(0,14), card(0,3)], &[card(0,5), card(0,7), card(0,9), card(1,2), card(2,4)]);
        let st = NlheEvaluator.evaluate_hand(&[card(1,9), card(2,8)], &[card(3,7), card(0,6), card(1,5), card(2,2), card(3,3)]);
        assert!(fl < st, "Flush should beat straight");
    }

    #[test]
    fn test_wheel_straight() {
        // A-2-3-4-5 wheel vs 3-4-5-6-7 straight (7-high). Wheel loses.
        let wheel = NlheEvaluator.evaluate_hand(&[card(0,14), card(1,2)], &[card(2,3), card(3,4), card(0,5), card(1,9), card(2,10)]);
        let straight7 = NlheEvaluator.evaluate_hand(&[card(0,6), card(1,7)], &[card(2,3), card(3,4), card(0,5), card(1,9), card(2,10)]);
        assert!(wheel > straight7, "Wheel (5-high) should lose to 7-high straight");
    }

    #[test]
    fn test_quads_vs_full_house() {
        let q = NlheEvaluator.evaluate_hand(&[card(0,8), card(1,8)], &[card(2,8), card(3,8), card(0,14), card(1,2), card(2,3)]);
        let fh = NlheEvaluator.evaluate_hand(&[card(0,14), card(1,14)], &[card(2,14), card(3,8), card(0,8), card(1,2), card(2,3)]);
        assert!(q < fh, "Quads should beat full house");
    }

    #[test]
    fn test_kicker_matters() {
        let r1 = NlheEvaluator.evaluate_hand(&[card(0,14), card(1,13)], &[card(2,5), card(3,7), card(0,9), card(1,2), card(2,3)]);
        let r2 = NlheEvaluator.evaluate_hand(&[card(0,14), card(1,12)], &[card(2,5), card(3,7), card(0,9), card(1,2), card(2,3)]);
        assert!(r1 < r2, "AK should beat AQ");
    }

    #[test]
    fn test_same_hand_ties() {
        let r1 = NlheEvaluator.evaluate_hand(&[card(0,14), card(1,13)], &[card(0,5), card(1,7), card(2,9), card(3,2), card(0,3)]);
        let r2 = NlheEvaluator.evaluate_hand(&[card(2,14), card(3,13)], &[card(0,5), card(1,7), card(2,9), card(3,2), card(0,3)]);
        assert_eq!(r1, r2, "Same hand should tie");
    }
}
