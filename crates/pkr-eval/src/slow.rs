use pkr_contracts::Evaluator;
// Card encoding: 0=2, 1=3, ..., 8=T, 9=J, 10=Q, 11=K, 12=A. Suit order: 0=Spade,1=Heart,2=Diamond,3=Club.
// Hand rank uses inverted bits (lower = better), so best hand has smallest u32 after `!raw`.

fn card_suit_rank(c: u8) -> (usize, usize) {
    let suit = (c / 13) as usize;
    let rank = (c % 13) as usize;
    (suit, rank)
}

fn eval_5(hand: &[u8; 5]) -> u32 {
    let mut rank_bits: u16 = 0;
    let mut suit_counts = [0u8; 4];
    let mut suit_ranks = [0u16; 4];
    let mut rank_counts = [0u8; 13];
    let mut ranks: [u8; 5] = [0; 5];

    for (i, &c) in hand.iter().enumerate() {
        if c == 255 {
            continue;
        }
        let (s, r) = card_suit_rank(c);
        suit_counts[s] += 1;
        suit_ranks[s] |= 1 << r;
        rank_counts[r] += 1;
        rank_bits |= 1 << r;
        ranks[i] = r as u8;
    }
    ranks.sort_unstable_by(|a, b| b.cmp(a));

    let flush_suit = suit_counts.iter().position(|&c| c >= 5);
    let mut straight_high = None;
    let mask = rank_bits;
    if mask >= 0x1F {
        let mut cnt = 0u8;
        for r in (0..13).rev() {
            if (mask & (1 << r)) != 0 {
                cnt += 1;
                if cnt >= 5 {
                    straight_high = Some(r as i8 + 4);
                    break;
                }
            } else {
                cnt = 0;
            }
        }
    }
    if straight_high.is_none() && (mask & 0x100F) == 0x100F {
        straight_high = Some(3);
    }
    if let Some(fs) = flush_suit {
        let fmask = suit_ranks[fs];
        let mut sf_high = None;
        let mut cnt = 0u8;
        for r in (0..13).rev() {
            if (fmask & (1 << r)) != 0 {
                cnt += 1;
                if cnt >= 5 {
                    sf_high = Some(r as i8 + 4);
                    break;
                }
            } else {
                cnt = 0;
            }
        }
        if sf_high.is_none() && (fmask & 0x100F) == 0x100F {
            sf_high = Some(3);
        }
        if let Some(high) = sf_high {
            let raw = (8u32 << 20) | ((high as u32) << 16);
            return !raw;
        }
    }
    if let Some(q) = rank_counts.iter().position(|&c| c == 4) {
        let quad_rank = q as u8;
        let kicker = ranks
            .iter()
            .find(|&&r| r as usize != q)
            .copied()
            .unwrap_or(0);
        let raw = (7u32 << 20) | ((quad_rank as u32) << 16) | ((kicker as u32) << 12);
        return !raw;
    }
    let trips = rank_counts.iter().position(|&c| c == 3);
    let pair = rank_counts.iter().position(|&c| c == 2);
    if let (Some(t), Some(p)) = (trips, pair) {
        let raw = (6u32 << 20) | ((t as u32) << 16) | ((p as u32) << 12);
        return !raw;
    }
    if let Some(fs) = flush_suit {
        let mut flush_ranks = [0u8; 5];
        let fmask = suit_ranks[fs];
        let mut idx = 0;
        for r in (0..13).rev() {
            if (fmask & (1 << r)) != 0 {
                flush_ranks[idx] = r as u8;
                idx += 1;
                if idx == 5 {
                    break;
                }
            }
        }
        let raw = (5u32 << 20)
            | ((flush_ranks[0] as u32) << 16)
            | ((flush_ranks[1] as u32) << 12)
            | ((flush_ranks[2] as u32) << 8)
            | ((flush_ranks[3] as u32) << 4)
            | (flush_ranks[4] as u32);
        return !raw;
    }
    if let Some(high) = straight_high {
        let raw = (4u32 << 20) | ((high as u32) << 16);
        return !raw;
    }
    if let Some(t) = trips {
        let mut kickers = [0u8; 2];
        let mut ki = 0;
        for &r in ranks.iter() {
            if r as usize != t {
                kickers[ki] = r;
                ki += 1;
                if ki == 2 {
                    break;
                }
            }
        }
        let raw = (3u32 << 20)
            | ((t as u32) << 16)
            | ((kickers[0] as u32) << 12)
            | ((kickers[1] as u32) << 8);
        return !raw;
    }
    let pairs: Vec<usize> = rank_counts
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c == 2)
        .map(|(i, _)| i)
        .collect();
    if pairs.len() >= 2 {
        // pairs[] is ascending by rank index; the HIGH pair must occupy the
        // high bits so that hands compare on the top pair first (audit F3).
        let p_high = pairs[pairs.len() - 1] as u8;
        let p_low = pairs[pairs.len() - 2] as u8;
        let kicker = ranks
            .iter()
            .find(|&&r| r != p_high && r != p_low)
            .copied()
            .unwrap_or(0);
        let raw = (2u32 << 20)
            | ((p_high as u32) << 16)
            | ((p_low as u32) << 12)
            | ((kicker as u32) << 8);
        return !raw;
    }
    if let Some(&p) = pairs.first() {
        let mut kickers = [0u8; 3];
        let mut ki = 0;
        for &r in ranks.iter() {
            if r as usize != p {
                kickers[ki] = r;
                ki += 1;
                if ki == 3 {
                    break;
                }
            }
        }
        let raw = (1u32 << 20)
            | ((p as u32) << 16)
            | ((kickers[0] as u32) << 12)
            | ((kickers[1] as u32) << 8)
            | ((kickers[2] as u32) << 4);
        return !raw;
    }
    let raw = ((ranks[0] as u32) << 16)
        | ((ranks[1] as u32) << 12)
        | ((ranks[2] as u32) << 8)
        | ((ranks[3] as u32) << 4)
        | (ranks[4] as u32);
    !raw
}

pub struct NlheEvaluator;

impl Evaluator for NlheEvaluator {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
        let mut cards = [255u8; 7];
        let mut idx = 0;

        // Filter out sentinel values (≥52) AND duplicate cards
        for &c in hole.iter().chain(board) {
            if c < 52 && !cards[..idx].contains(&c) {
                cards[idx] = c;
                idx += 1;
            }
        }

        let total = idx;
        if total < 5 {
            return u32::MAX;
        }

        const COMBOS_7_5: [[u8; 5]; 21] = [
            [0, 1, 2, 3, 4],
            [0, 1, 2, 3, 5],
            [0, 1, 2, 3, 6],
            [0, 1, 2, 4, 5],
            [0, 1, 2, 4, 6],
            [0, 1, 2, 5, 6],
            [0, 1, 3, 4, 5],
            [0, 1, 3, 4, 6],
            [0, 1, 3, 5, 6],
            [0, 1, 4, 5, 6],
            [0, 2, 3, 4, 5],
            [0, 2, 3, 4, 6],
            [0, 2, 3, 5, 6],
            [0, 2, 4, 5, 6],
            [0, 3, 4, 5, 6],
            [1, 2, 3, 4, 5],
            [1, 2, 3, 4, 6],
            [1, 2, 3, 5, 6],
            [1, 2, 4, 5, 6],
            [1, 3, 4, 5, 6],
            [2, 3, 4, 5, 6],
        ];
        let mut best = u32::MAX;
        let num = match total {
            5 => 1,
            6 => 6,
            7 => 21,
            _ => 0,
        };
        for combo in COMBOS_7_5.iter().take(num) {
            let mut h = [0u8; 5];
            for (j, &ci) in combo.iter().enumerate() {
                h[j] = cards[ci as usize];
            }
            let r = eval_5(&h);
            if r < best {
                best = r;
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Card id encoding (see file header): id = suit*13 + rank, rank 0..=12.
    // Helper: evaluate 5 cards given (rank, suit) pairs.
    fn ev(cards: [(usize, usize); 5]) -> u32 {
        let arr: [u8; 5] = cards.map(|(r, s)| (s * 13 + r) as u8);
        NlheEvaluator.evaluate_hand(&arr, &[])
    }

    #[test]
    fn two_pair_compares_on_high_pair_first() {
        // K K 4 4 7  must beat  Q Q 2 2 9   (rank idx: K=11, 4=2, 7=5, Q=10, 2=0)
        let kk447 = ev([(11, 0), (11, 1), (2, 0), (2, 1), (5, 0)]);
        let qq229 = ev([(10, 0), (10, 1), (0, 0), (0, 1), (7, 0)]);
        assert!(kk447 < qq229, "kings-up must rank better (lower) than queens-up");
    }

    #[test]
    fn two_pair_compares_on_low_pair_when_high_ties() {
        // A A 9 9 2 must beat  A A 8 8 K  (rank idx: A=12, 9=7, 2=0, 8=6, K=11)
        let aa992 = ev([(12, 0), (12, 1), (7, 0), (7, 1), (0, 0)]);
        let aa88k = ev([(12, 2), (12, 3), (6, 2), (6, 3), (11, 2)]);
        assert!(aa992 < aa88k);
    }

    #[test]
    fn two_pair_kicker_breaks_ties() {
        // A A 7 7 K  beats  A A 7 7 Q
        let a77k = ev([(12, 0), (12, 1), (5, 0), (5, 1), (11, 0)]);
        let a77q = ev([(12, 2), (12, 3), (5, 2), (5, 3), (10, 2)]);
        assert!(a77k < a77q);
    }
}
