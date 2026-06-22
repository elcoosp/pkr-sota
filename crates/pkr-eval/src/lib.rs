use pkr_contracts::Evaluator;

pub struct NlheEvaluator;

impl Evaluator for NlheEvaluator {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
        let mut cards = [255u8; 7];
        let mut idx = 0;
        for &c in hole {
            if idx < 7 {
                cards[idx] = c;
                idx += 1;
            }
        }
        for &c in board {
            if idx < 7 {
                cards[idx] = c;
                idx += 1;
            }
        }
        eval_7_cards(&cards)
    }
}

#[inline]
fn eval_7_cards(cards: &[u8; 7]) -> u32 {
    let mut rank_counts = [0u8; 13];
    let mut suit_counts = [0u8; 4];
    let mut suit_ranks = [0u16; 4];
    let mut rank_mask = 0u16;

    for &c in cards.iter() {
        if c == 255 {
            continue;
        }
        let suit = (c / 13) as usize;
        let rank = (c % 13) as usize;
        rank_counts[rank] += 1;
        suit_counts[suit] += 1;
        suit_ranks[suit] |= 1 << rank;
        rank_mask |= 1 << rank;
    }

    let mut flush_suit = None;
    for s in 0..4 {
        if suit_counts[s] >= 5 {
            flush_suit = Some(s);
            break;
        }
    }

    let mut straight_high = -1i32;
    let mut count = 0;
    for r in (0..13i32).rev() {
        if (rank_mask & (1 << r)) != 0 {
            count += 1;
            if count >= 5 {
                straight_high = r;
                break;
            }
        } else {
            count = 0;
        }
    }
    if straight_high == -1 {
        if (rank_mask & (1 << 12)) != 0 && (rank_mask & 0xF) == 0xF {
            straight_high = 3;
        } // Wheel
    }

    if let Some(fs) = flush_suit {
        let fr = suit_ranks[fs];
        let mut sf_high = -1i32;
        let mut c = 0;
        for r in (0..13i32).rev() {
            if (fr & (1 << r)) != 0 {
                c += 1;
                if c >= 5 {
                    sf_high = r;
                    break;
                }
            } else {
                c = 0;
            }
        }
        if sf_high == -1 {
            if (fr & (1 << 12)) != 0 && (fr & 0xF) == 0xF {
                sf_high = 3;
            }
        }
        if sf_high != -1 {
            return rank_value(8, sf_high as u8, 0, 0, 0, 0);
        }
    }

    let mut quads = -1i32;
    let mut k1 = -1i32;
    for r in (0..13i32).rev() {
        if rank_counts[r as usize] == 4 {
            quads = r;
        } else if rank_counts[r as usize] > 0 && k1 == -1 {
            k1 = r;
        }
    }
    if quads != -1 {
        return rank_value(7, quads as u8, k1 as u8, 0, 0, 0);
    }

    let mut trips = -1i32;
    let mut pair = -1i32;
    for r in (0..13i32).rev() {
        if rank_counts[r as usize] == 3 {
            if trips == -1 {
                trips = r;
            } else if pair == -1 {
                pair = r;
            }
        } else if rank_counts[r as usize] == 2 {
            if pair == -1 {
                pair = r;
            }
        }
    }
    if trips != -1 && pair != -1 {
        return rank_value(6, trips as u8, pair as u8, 0, 0, 0);
    }

    if let Some(fs) = flush_suit {
        let fr = suit_ranks[fs];
        let mut kickers = [0u8; 5];
        let mut k_idx = 0;
        for r in (0..13i32).rev() {
            if (fr & (1 << r)) != 0 {
                kickers[k_idx] = r as u8;
                k_idx += 1;
                if k_idx == 5 {
                    break;
                }
            }
        }
        return rank_value(
            5, kickers[0], kickers[1], kickers[2], kickers[3], kickers[4],
        );
    }

    if straight_high != -1 {
        return rank_value(4, straight_high as u8, 0, 0, 0, 0);
    }

    if trips != -1 {
        let mut kickers = [0u8; 2];
        let mut k_idx = 0;
        for r in (0..13i32).rev() {
            if rank_counts[r as usize] > 0 && r != trips {
                kickers[k_idx] = r as u8;
                k_idx += 1;
                if k_idx == 2 {
                    break;
                }
            }
        }
        return rank_value(3, trips as u8, kickers[0], kickers[1], 0, 0);
    }

    let mut pairs = [-1i32; 2];
    let mut p_idx = 0;
    for r in (0..13i32).rev() {
        if rank_counts[r as usize] == 2 {
            pairs[p_idx] = r;
            p_idx += 1;
            if p_idx == 2 {
                break;
            }
        }
    }
    if pairs[0] != -1 && pairs[1] != -1 {
        let mut k = 0u8;
        for r in (0..13i32).rev() {
            if rank_counts[r as usize] > 0 && r != pairs[0] && r != pairs[1] {
                k = r as u8;
                break;
            }
        }
        return rank_value(2, pairs[0] as u8, pairs[1] as u8, k, 0, 0);
    }

    if pairs[0] != -1 {
        let mut kickers = [0u8; 3];
        let mut k_idx = 0;
        for r in (0..13i32).rev() {
            if rank_counts[r as usize] > 0 && r != pairs[0] {
                kickers[k_idx] = r as u8;
                k_idx += 1;
                if k_idx == 3 {
                    break;
                }
            }
        }
        return rank_value(1, pairs[0] as u8, kickers[0], kickers[1], kickers[2], 0);
    }

    let mut kickers = [0u8; 5];
    let mut k_idx = 0;
    for r in (0..13i32).rev() {
        if rank_counts[r as usize] > 0 {
            kickers[k_idx] = r as u8;
            k_idx += 1;
            if k_idx == 5 {
                break;
            }
        }
    }
    rank_value(
        0, kickers[0], kickers[1], kickers[2], kickers[3], kickers[4],
    )
}

#[inline]
fn rank_value(cat: u8, k1: u8, k2: u8, k3: u8, k4: u8, k5: u8) -> u32 {
    ((cat as u32) << 20)
        | ((k1 as u32) << 16)
        | ((k2 as u32) << 12)
        | ((k3 as u32) << 8)
        | ((k4 as u32) << 4)
        | (k5 as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
