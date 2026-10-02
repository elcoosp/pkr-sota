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
    // T8: fixed-size stack array instead of Vec<usize>. A 5-card hand
    // can have at most 2 pairs, so 3 slots is generous. Removes a
    // heap allocation per eval_5 call (21 calls per evaluate_hand on a
    // 7-card river hand).
    let mut pairs = [0usize; 3];
    let mut npairs = 0usize;
    for (i, &c) in rank_counts.iter().enumerate() {
        if c == 2 {
            if npairs < 3 {
                pairs[npairs] = i;
            }
            npairs += 1;
        }
    }
    if npairs >= 2 {
        // pairs[] is ascending by rank index; the HIGH pair must occupy the
        // high bits so that hands compare on the top pair first (audit F3).
        let p_high = pairs[npairs - 1] as u8;
        let p_low = pairs[npairs - 2] as u8;
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
    if npairs == 1 {
        let p = pairs[0];
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
        debug_assert!(
            hole.len() + board.len() <= 7,
            "evaluate_hand: {} cards exceeds the 7-card buffer",
            hole.len() + board.len()
        );
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

        // All 5-of-7 combinations (C(7,5) = 21).
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
        // All 5-of-6 combinations (C(6,5) = 6). Previously the code
        // reused the first 6 rows of COMBOS_7_5, which reference index
        // 6 (the sentinel 255) and miss the combinations that omit
        // cards 0, 1, 2 — giving wrong results for any 6-card input.
        const COMBOS_6_5: [[u8; 5]; 6] = [
            [1, 2, 3, 4, 5], // omit 0
            [0, 2, 3, 4, 5], // omit 1
            [0, 1, 3, 4, 5], // omit 2
            [0, 1, 2, 4, 5], // omit 3
            [0, 1, 2, 3, 5], // omit 4
            [0, 1, 2, 3, 4], // omit 5
        ];
        const COMBOS_5_5: [[u8; 5]; 1] = [[0, 1, 2, 3, 4]];

        let table: &[[u8; 5]] = match total {
            5 => &COMBOS_5_5,
            6 => &COMBOS_6_5,
            7 => &COMBOS_7_5,
            _ => &COMBOS_5_5[..0],
        };

        let mut best = u32::MAX;
        for combo in table {
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
        assert!(
            kk447 < qq229,
            "kings-up must rank better (lower) than queens-up"
        );
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

#[cfg(test)]
mod audit_f3_tests {
    use super::*;

    /// Encode a card as (rank, suit) → u8 with the same layout the file
    /// header documents: id = suit * 13 + rank, ranks 2..=A = 0..=12.
    #[inline]
    fn card(rank: u8, suit: u8) -> u8 {
        debug_assert!(rank < 13 && suit < 4);
        suit * 13 + rank
    }

    /// The exact hand that motivated audit F3: `K K 4 4 7` must rank
    /// *better* (lower u32) than `Q Q 2 2 9`. The buggy version put the
    /// weaker pair in the high bits, so KK447's raw was *larger* than
    /// QQ229's raw and hence ranked worse.
    ///
    /// Ranks: 2=0, 3=1, 4=2, 5=3, 6=4, 7=5, 8=6, 9=7, T=8, J=9, Q=10, K=11, A=12.
    #[test]
    fn kings_up_beats_queens_up() {
        let ev = NlheEvaluator;
        let kk447 = [
            card(11, 0),
            card(11, 1), // KK
            card(2, 0),
            card(2, 1), // 44
            card(5, 0), // 7
        ];
        let qq229 = [
            card(10, 0),
            card(10, 1), // QQ
            card(0, 0),
            card(0, 1), // 22
            card(7, 0), // 9
        ];
        let r_kk = ev.evaluate_hand(&kk447, &[]);
        let r_qq = ev.evaluate_hand(&qq229, &[]);
        assert!(
            r_kk < r_qq,
            "K K 4 4 7 must rank better (lower) than Q Q 2 2 9; got KK447={r_kk}, QQ229={r_qq}",
        );
    }

    /// When the high pair ties, the low pair decides the order. `A A 9 9 2`
    /// must beat `A A 8 8 K`.
    #[test]
    fn low_pair_breaks_ties_when_high_ties() {
        let ev = NlheEvaluator;
        let aa992 = [
            card(12, 0),
            card(12, 1), // AA
            card(7, 0),
            card(7, 1), // 99
            card(0, 0), // 2
        ];
        let aa88k = [
            card(12, 2),
            card(12, 3), // AA
            card(6, 2),
            card(6, 3),  // 88
            card(11, 2), // K
        ];
        let r1 = ev.evaluate_hand(&aa992, &[]);
        let r2 = ev.evaluate_hand(&aa88k, &[]);
        assert!(r1 < r2, "AA992 must beat AA88K; got {r1} vs {r2}");
    }

    /// Kicker only matters when both pairs tie. `A A 7 7 K` beats `A A 7 7 Q`.
    #[test]
    fn kicker_breaks_ties_when_both_pairs_tie() {
        let ev = NlheEvaluator;
        let a77k = [
            card(12, 0),
            card(12, 1), // AA
            card(5, 0),
            card(5, 1),  // 77
            card(11, 0), // K
        ];
        let a77q = [
            card(12, 2),
            card(12, 3), // AA
            card(5, 2),
            card(5, 3),  // 77
            card(10, 2), // Q
        ];
        let r1 = ev.evaluate_hand(&a77k, &[]);
        let r2 = ev.evaluate_hand(&a77q, &[]);
        assert!(r1 < r2, "A A 7 7 K must beat A A 7 7 Q; got {r1} vs {r2}");
    }

    /// Cross-check: two-pair must still rank worse than trips and better
    /// than one pair. Ensures the fix didn't invert the whole category.
    #[test]
    fn two_pair_sits_between_one_pair_and_trips() {
        let ev = NlheEvaluator;
        let two_pair = [
            card(12, 0),
            card(12, 1), // AA
            card(11, 0),
            card(11, 1), // KK
            card(9, 0),  // J
        ];
        let trips = [
            card(12, 2),
            card(12, 3), // A♠ A♥  (pair)
            card(12, 0), // A♦  (trips)
            card(5, 0),
            card(3, 0), // 7 5  kickers
        ];
        let one_pair = [
            card(12, 0),
            card(12, 1), // AA
            card(9, 0),
            card(8, 0),
            card(6, 0), // J T 8
        ];
        let r_tp = ev.evaluate_hand(&two_pair, &[]);
        let r_t = ev.evaluate_hand(&trips, &[]);
        let r_op = ev.evaluate_hand(&one_pair, &[]);
        assert!(r_t < r_tp, "trips must rank better than two-pair");
        assert!(r_tp < r_op, "two-pair must rank better than one-pair");
    }

    /// TableEvaluator on the shipped hand_ranks.bin must agree with the
    /// slow evaluator on the F3-sensitive hands. Skipped by default
    /// because it needs a table file on disk; run with:
    ///   PKR_RANK_TABLE=outputs/v17/hand_ranks.bin \
    ///     cargo test -p pkr-eval -- --ignored f3_table_agrees
    #[test]
    #[ignore = "requires PKR_RANK_TABLE pointing at a regenerated hand_ranks.bin"]
    fn f3_table_agrees_with_slow() {
        let path = match std::env::var("PKR_RANK_TABLE") {
            Ok(p) => p,
            Err(_) => {
                eprintln!("PKR_RANK_TABLE not set; skipping");
                return;
            }
        };
        let table = crate::TableEvaluator::new(&path).expect("load hand_ranks.bin");
        let slow = NlheEvaluator;

        let kk447: [u8; 5] = [card(11, 0), card(11, 1), card(2, 0), card(2, 1), card(5, 0)];
        let qq229: [u8; 5] = [card(10, 0), card(10, 1), card(0, 0), card(0, 1), card(7, 0)];

        let t_kk = table.evaluate_hand(&kk447, &[]);
        let t_qq = table.evaluate_hand(&qq229, &[]);
        let s_kk = slow.evaluate_hand(&kk447, &[]);
        let s_qq = slow.evaluate_hand(&qq229, &[]);

        assert!(t_kk < t_qq, "table must rank KK447 better than QQ229");
        assert_eq!(t_kk, s_kk, "table vs slow disagreement on KK447");
        assert_eq!(t_qq, s_qq, "table vs slow disagreement on QQ229");
    }

    /// Differential: 10k random 5-card hands through TableEvaluator and
    /// NlheEvaluator must produce bit-identical ranks. Skipped unless
    /// PKR_RANK_TABLE is set.
    #[test]
    #[ignore = "requires PKR_RANK_TABLE pointing at a regenerated hand_ranks.bin"]
    fn f3_table_and_slow_agree_on_random_hands() {
        let path = match std::env::var("PKR_RANK_TABLE") {
            Ok(p) => p,
            Err(_) => {
                eprintln!("PKR_RANK_TABLE not set; skipping");
                return;
            }
        };
        let table = crate::TableEvaluator::new(&path).expect("load hand_ranks.bin");
        let slow = NlheEvaluator;

        // Deterministic LCG so failures are reproducible.
        let mut seed: u64 = 0xF3F3_F3F3_F3F3_F3F3;
        let mut next = || -> u8 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) & 0xFF) as u8
        };

        let mut mismatches = 0u32;
        for i in 0..10_000 {
            let mut used = [false; 52];
            let mut cards = [0u8; 5];
            let mut n = 0usize;
            while n < 5 {
                let c = (next() % 52) as usize;
                if !used[c] {
                    used[c] = true;
                    cards[n] = c as u8;
                    n += 1;
                }
            }
            let t = table.evaluate_hand(&cards, &[]);
            let s = slow.evaluate_hand(&cards, &[]);
            if t != s {
                mismatches += 1;
                if mismatches <= 3 {
                    eprintln!("mismatch at iter {i}: cards={cards:?} table={t} slow={s}");
                }
            }
        }
        assert_eq!(
            mismatches, 0,
            "{mismatches} table/slow mismatches out of 10k"
        );
    }

    /// F4-bug regression: 6-card inputs must evaluate every 5-of-6
    /// combination, not the first 6 rows of the 7-card table (which
    /// reference the sentinel index 6 and skip combos).
    ///
    /// Constructs a known-best 5-card subset of a 6-card hand and
    /// asserts slow.rs finds it. Without the fix, the best 5-card
    /// subset that omits cards 0, 1 or 2 would be missed.
    #[test]
    fn six_card_inputs_see_every_subset() {
        let slow = super::NlheEvaluator;
        // Six cards: A K Q J T 2, suits arranged so the AKQJT is a
        // straight. The best 5-card subset is AKQJT (omitting the 2).
        // Card encoding: suit*13 + rank, rank 0=Two .. 12=Ace.
        // Put AKQJT in mixed suits so only the rank pattern matters.
        let six = [12u8, 11, 10, 9, 8, 0]; // A K Q J T 2, all suit 0
        let r6 = slow.evaluate_hand(&six, &[]);
        // Compare against the same hand minus the 2 (5 cards).
        let five = [12u8, 11, 10, 9, 8];
        let r5 = slow.evaluate_hand(&five, &[]);
        assert_eq!(
            r6, r5,
            "6-card hand must find the same best 5-card subset;              six={six:?} -> {r6}, five={five:?} -> {r5}"
        );
    }

    /// Second regression case: the 6-card input must not panic on the
    /// sentinel-255 index that the old code would have read.
    #[test]
    fn six_card_inputs_do_not_panic() {
        let slow = super::NlheEvaluator;
        let six = [0u8, 14, 28, 42, 3, 7];
        let _ = slow.evaluate_hand(&six, &[]);
    }
}
