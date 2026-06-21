use std::collections::HashMap;
use std::sync::LazyLock;

/// Pack 5 sorted u8 card indices (0-51) into a single u64.
#[inline]
fn pack_hand(hand: &[u8; 5]) -> u64 {
    let mut packed: u64 = 0;
    for &card in hand.iter() {
        packed = (packed << 6) | (card as u64);
    }
    packed
}

/// Static lookup table built lazily on first access.
static HAND_RANKS: LazyLock<HashMap<u64, u16>> = LazyLock::new(build_hand_rank_table);

fn build_hand_rank_table() -> HashMap<u64, u16> {
    let mut hands_with_keys: Vec<(u64, u32)> = Vec::with_capacity(2598960);

    for a in 0..48u8 {
        for b in (a + 1)..49 {
            for c in (b + 1)..50 {
                for d in (c + 1)..51 {
                    for e in (d + 1)..52 {
                        let hand = [a, b, c, d, e];
                        let strength = compute_strength_key(&hand);
                        let packed = pack_hand(&hand);
                        hands_with_keys.push((packed, strength));
                    }
                }
            }
        }
    }

    hands_with_keys.sort_by(|a, b| a.1.cmp(&b.1));

    let mut table = HashMap::with_capacity(hands_with_keys.len());
    let mut current_rank: u16 = 0;
    let mut prev_strength: Option<u32> = None;

    for (packed, strength) in &hands_with_keys {
        if prev_strength != Some(*strength) {
            if prev_strength.is_some() {
                current_rank += 1;
            }
            prev_strength = Some(*strength);
        }
        table.insert(*packed, current_rank);
    }

    table
}

/// Compute a strength key for a 5‑card hand (cards sorted by index).
/// Lower key = stronger hand.  Two hands with identical poker value get the same key.
fn compute_strength_key(hand: &[u8; 5]) -> u32 {
    let suits: [u8; 5] = [
        hand[0] / 13,
        hand[1] / 13,
        hand[2] / 13,
        hand[3] / 13,
        hand[4] / 13,
    ];
    let mut ranks: [u8; 5] = [
        hand[0] % 13,
        hand[1] % 13,
        hand[2] % 13,
        hand[3] % 13,
        hand[4] % 13,
    ];
    ranks.sort_unstable();

    let is_flush = suits[0] == suits[1]
        && suits[1] == suits[2]
        && suits[2] == suits[3]
        && suits[3] == suits[4];

    let is_wheel = ranks == [0, 1, 2, 3, 12];
    let is_straight = is_wheel
        || (ranks[1] == ranks[0] + 1
            && ranks[2] == ranks[1] + 1
            && ranks[3] == ranks[2] + 1
            && ranks[4] == ranks[3] + 1);

    let straight_high = if is_wheel {
        3
    } else if is_straight {
        ranks[4]
    } else {
        0
    };

    let mut freq = [0u8; 13];
    for &r in &ranks {
        freq[r as usize] += 1;
    }

    let mut quads: Option<u8> = None;
    let mut trips: Option<u8> = None;
    let mut pair_high: Option<u8> = None;
    let mut pair_low: Option<u8> = None;
    for r in (0u8..13).rev() {
        match freq[r as usize] {
            4 => quads = Some(r),
            3 => trips = Some(r),
            2 => {
                if pair_high.is_none() {
                    pair_high = Some(r);
                } else {
                    pair_low = Some(r);
                }
            }
            _ => {}
        }
    }

    let kickers_desc = || -> Vec<u8> {
        let mut k: Vec<u8> = ranks
            .iter()
            .filter(|&&r| freq[r as usize] == 1)
            .cloned()
            .collect();
        k.sort_by(|a, b| b.cmp(a));
        k
    };

    let category: u32;
    let within: u32;

    if is_straight && is_flush {
        category = 0;
        within = if is_wheel {
            9
        } else {
            12 - straight_high as u32
        };
    } else if let Some(q) = quads {
        category = 1;
        let kicker = kickers_desc()[0];
        within = (12 - q as u32) * 13 + (12 - kicker as u32);
    } else if let Some(t) = trips {
        if let Some(p) = pair_high {
            category = 2; // Full house
            within = (12 - t as u32) * 13 + (12 - p as u32);
        } else {
            category = 5; // Three of a kind
            let kickers = kickers_desc();
            within =
                (12 - t as u32) * 169 + (12 - kickers[0] as u32) * 13 + (12 - kickers[1] as u32);
        }
    } else if is_flush {
        category = 3;
        let mut r_sorted = ranks;
        r_sorted.sort_by(|a, b| b.cmp(a));
        let mut val: u32 = 0;
        for &r in &r_sorted {
            val = val * 13 + (12 - r as u32);
        }
        within = val;
    } else if is_straight {
        category = 4;
        within = if is_wheel {
            9
        } else {
            12 - straight_high as u32
        };
    } else if let Some(ph) = pair_high {
        if let Some(pl) = pair_low {
            category = 6; // Two pair
            let kicker = kickers_desc()[0];
            within = (12 - ph as u32) * 169 + (12 - pl as u32) * 13 + (12 - kicker as u32);
        } else {
            category = 7; // One pair
            let kickers = kickers_desc();
            within = (12 - ph as u32) * 2197
                + (12 - kickers[0] as u32) * 169
                + (12 - kickers[1] as u32) * 13
                + (12 - kickers[2] as u32);
        }
    } else {
        category = 8; // High card
        let mut r_sorted = ranks;
        r_sorted.sort_by(|a, b| b.cmp(a));
        let mut val: u32 = 0;
        for &r in &r_sorted {
            val = val * 13 + (12 - r as u32);
        }
        within = val;
    }

    (category << 28) | within
}

/// Lookup the rank of a 5‑card hand (must be sorted by card index). 0 = best.
pub fn five_card_rank(mut hand: [u8; 5]) -> u16 {
    // Sort required for pack_hand lookup
    hand.sort_unstable();
    let packed = pack_hand(&hand);
    HAND_RANKS[&packed]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_builds_without_panic() {
        let rank = five_card_rank([0, 1, 2, 3, 4]);
        assert!(rank > 0, "Rank should be > 0 for a weak hand (7-high)");
    }

    #[test]
    fn royal_flush_rank_is_zero() {
        let hand = [8, 9, 10, 11, 12]; // 10♠ J♠ Q♠ K♠ A♠ (spades)
        let rank = five_card_rank(hand);
        assert_eq!(rank, 0, "Royal flush should have rank 0, got {}", rank);
    }

    #[test]
    fn wheel_straight_flush_rank() {
        let hand = [13, 14, 15, 16, 25]; // 2♥ 3♥ 4♥ 5♥ A♥ (hearts)
        let rank = five_card_rank(hand);
        assert_eq!(
            rank, 9,
            "Wheel straight flush should have rank 9, got {}",
            rank
        );
    }

    #[test]
    fn pack_unpack_consistency() {
        let hand = [0, 10, 20, 30, 40];
        let _packed = pack_hand(&hand);
        assert_eq!(pack_hand(&hand), pack_hand(&hand));
    }

    #[test]
    fn different_suits_same_rank_straight_flush() {
        let hearts = [21, 22, 23, 24, 25]; // A♥ K♥ Q♥ J♥ 10♥
        let rank = five_card_rank(hearts);
        assert_eq!(rank, 0, "Hearts royal flush should be rank 0, got {}", rank);
    }

    #[test]
    fn same_value_hands_tie() {
        let sf1 = [8, 9, 10, 11, 12];
        let sf2 = [8, 9, 10, 11, 12];
        assert_eq!(five_card_rank(sf1), five_card_rank(sf2));
    }

    #[test]
    fn full_house_ranks_ordered() {
        let mut aces_full = [
            0 * 13 + 12, // A♠ (12)
            0 * 13 + 11, // K♠ (11)
            1 * 13 + 12, // A♥ (25)
            1 * 13 + 11, // K♥ (24)
            2 * 13 + 12, // A♦ (38)
        ];
        aces_full.sort_unstable(); // [11,12,24,25,38]

        let mut kings_full = [
            0 * 13 + 11, // K♠ (11)
            0 * 13 + 12, // A♠ (12)
            1 * 13 + 11, // K♥ (24)
            1 * 13 + 12, // A♥ (25)
            2 * 13 + 11, // K♦ (37)
        ];
        kings_full.sort_unstable(); // [11,12,24,25,37]

        assert!(
            five_card_rank(aces_full) < five_card_rank(kings_full),
            "Aces full of Kings should beat Kings full of Aces"
        );
    }

    #[test]
    fn quads_higher_rank_better() {
        let mut quad_aces = [
            0 * 13 + 12, // A♠ (12)
            0 * 13 + 11, // K♠ (11)
            1 * 13 + 12, // A♥ (25)
            2 * 13 + 12, // A♦ (38)
            3 * 13 + 12, // A♣ (51)
        ];
        quad_aces.sort_unstable(); // [11,12,25,38,51]

        let mut quad_kings = [
            0 * 13 + 11, // K♠ (11)
            0 * 13 + 12, // A♠ (12)
            1 * 13 + 11, // K♥ (24)
            2 * 13 + 11, // K♦ (37)
            3 * 13 + 11, // K♣ (50)
        ];
        quad_kings.sort_unstable(); // [11,12,24,37,50]

        assert!(
            five_card_rank(quad_aces) < five_card_rank(quad_kings),
            "Quad Aces should beat Quad Kings"
        );
    }

    #[test]
    fn flush_ranked_by_highest_card() {
        let mut flush1 = [
            1 * 13 + 12, // A♥ (25)
            1 * 13 + 10, // Q♥ (23)
            1 * 13 + 8,  // 10♥ (21)
            1 * 13 + 6,  // 8♥ (19)
            1 * 13 + 4,  // 6♥ (17)
        ];
        flush1.sort_unstable(); // [17,19,21,23,25]

        let mut flush2 = [
            1 * 13 + 12, // A♥ (25)
            1 * 13 + 10, // Q♥ (23)
            1 * 13 + 8,  // 10♥ (21)
            1 * 13 + 6,  // 8♥ (19)
            1 * 13 + 3,  // 5♥ (16)
        ];
        flush2.sort_unstable(); // [16,19,21,23,25]

        assert!(
            five_card_rank(flush1) < five_card_rank(flush2),
            "Flush with higher fifth card should win"
        );
    }
}
