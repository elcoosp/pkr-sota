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
        let p1 = pairs[0] as u8;
        let p2 = pairs[1] as u8;
        let kicker = ranks
            .iter()
            .find(|&&r| r != p1 && r != p2)
            .copied()
            .unwrap_or(0);
        let raw = (2u32 << 20) | ((p1 as u32) << 16) | ((p2 as u32) << 12) | ((kicker as u32) << 8);
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
