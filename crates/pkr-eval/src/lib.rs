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
