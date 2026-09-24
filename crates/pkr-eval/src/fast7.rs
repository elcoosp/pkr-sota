//! Fast 7-card hand evaluator.
//!
//! Replaces the 21-subset enumeration in `TableEvaluator` with a single
//! rank-count lookup plus an optional flush check. Expected speedup:
//! 10-30x on the hot path (showdown eval, EHS precompute, BR checker).
//!
//! ## Design
//!
//! 1. Compute rank counts (13 values in 0..4) and suit counts (4 values).
//! 2. Look up the best *non-flush* 5-card rank in the `non_flush` LUT.
//! 3. If any suit has >= 5 cards, look up the flush rank directly in
//!    the 5-card LUT (top 5 of that suit), take the min.
//!
//! The `non_flush` LUT is precomputed at load time by enumerating all
//! valid rank multisets of size 5, 6, and 7, and computing for each the
//! best non-flush 5-card hand. Building it costs ~50 ms and ~30K entries.
//!
//! ## Output contract
//!
//! `Fast7Evaluator::evaluate_hand` MUST return bit-identical values to
//! `TableEvaluator::evaluate_hand` and `NlheEvaluator::evaluate_hand`.
//! Both are asserted by the differential tests below.

use crate::lookup::combinadic_rank;
use memmap2::Mmap;
use pkr_contracts::Evaluator;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

/// Fast 7-card evaluator using a precomputed rank-count LUT.
pub struct Fast7Evaluator {
    /// Mmap of `hand_ranks.bin`: C(52,5) u32 entries indexed by
    /// `combinadic_rank(sorted_desc_5_cards)`.
    mmap: Mmap,
    /// Rank-count key -> best non-flush rank.
    /// Key = sum over r in 0..13 of count[r] * 5^r (fits in u32 since
    /// 5^13 = 1_220_703_125 < 2^31).
    non_flush: HashMap<u32, u32>,
}

impl Fast7Evaluator {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let non_flush = precompute_non_flush(&mmap);
        Ok(Self { mmap, non_flush })
    }

    /// Number of distinct rank-multiset keys in the precomputed LUT.
    /// Expected in the 15K-30K range.
    pub fn non_flush_len(&self) -> usize {
        self.non_flush.len()
    }

    #[inline(always)]
    fn lut_rank(&self, sorted_desc: &[u8; 5]) -> u32 {
        let idx = combinadic_rank(sorted_desc) as usize;
        let offset = idx * 4;
        if offset + 4 > self.mmap.len() {
            return u32::MAX;
        }
        u32::from_le_bytes([
            self.mmap[offset],
            self.mmap[offset + 1],
            self.mmap[offset + 2],
            self.mmap[offset + 3],
        ])
    }
}

impl Evaluator for Fast7Evaluator {
    #[inline]
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
        let mut cards = [0u8; 7];
        let mut n = 0usize;
        for &c in hole.iter().chain(board.iter()) {
            if c < 52 && !cards[..n].contains(&c) {
                cards[n] = c;
                n += 1;
            }
        }
        if n < 5 {
            return u32::MAX;
        }

        let mut suit_counts = [0u8; 4];
        let mut rank_counts = [0u8; 13];
        for i in 0..n {
            let c = cards[i];
            // Card encoding matches the rest of the crate (slow.rs,
            // hand_ranks.bin, precompute): suit = c / 13, rank = c % 13.
            suit_counts[(c / 13) as usize] += 1;
            rank_counts[(c % 13) as usize] += 1;
        }

        let key = rank_key(&rank_counts);
        let mut best = *self.non_flush.get(&key).unwrap_or(&u32::MAX);

        // At most one suit can have >= 5 cards in a 7-card hand, so break
        // after the first match.
        //
        // NOTE: taking the top-5-by-card-id gives the best FLUSH, but the
        // best FLUSH-or-STRAIGHT-FLUSH may be a different 5-card subset
        // of the suited cards (e.g. spades A T 5 4 3 2 -> the top-5 by id
        // is the Ace-high flush, but A 5 4 3 2 is a wheel straight flush,
        // which is a strictly stronger hand). Enumerate every C(m,5)
        // subset of the suited cards and take the min (best) LUT rank.
        for s in 0..4u8 {
            if suit_counts[s as usize] < 5 {
                continue;
            }
            let mut suited = [0u8; 7];
            let mut m = 0usize;
            for i in 0..n {
                if (cards[i] / 13) == s {
                    suited[m] = cards[i];
                    m += 1;
                }
            }
            // m in 5..=7; sort descending so subset indices preserve order.
            suited[..m].sort_unstable_by(|a, b| b.cmp(a));

            // 5-card-subsets of m by index; reused pattern from slow.rs.
            const COMBOS_7_5: [[u8; 5]; 21] = [
                [0,1,2,3,4],[0,1,2,3,5],[0,1,2,3,6],[0,1,2,4,5],[0,1,2,4,6],[0,1,2,5,6],
                [0,1,3,4,5],[0,1,3,4,6],[0,1,3,5,6],[0,1,4,5,6],[0,2,3,4,5],[0,2,3,4,6],
                [0,2,3,5,6],[0,2,4,5,6],[0,3,4,5,6],[1,2,3,4,5],[1,2,3,4,6],[1,2,3,5,6],
                [1,2,4,5,6],[1,3,4,5,6],[2,3,4,5,6],
            ];
            // COMBOS_7_5 assumes 7 slots. For m=6, entries referencing
            // index 6 must be skipped (they would read zero-initialized
            // slots and duplicate cards). Filter by index < m.
            for combo in COMBOS_7_5.iter() {
                if combo.iter().any(|&i| (i as usize) >= m) {
                    continue;
                }
                let mut sel = [0u8; 5];
                for (j, &ci) in combo.iter().enumerate() {
                    sel[j] = suited[ci as usize];
                }
                let r = self.lut_rank(&sel);
                if r < best {
                    best = r;
                }
            }
            break;
        }

        best
    }
}

#[inline(always)]
fn rank_key(counts: &[u8; 13]) -> u32 {
    let mut k: u32 = 0;
    for r in 0..13 {
        k = k * 5 + counts[r] as u32;
    }
    k
}

fn precompute_non_flush(mmap: &Mmap) -> HashMap<u32, u32> {
    let mut map: HashMap<u32, u32> = HashMap::with_capacity(30_000);
    for total in 5..=7 {
        let mut counts = [0u8; 13];
        enumerate_counts(&mut counts, 0, total, &mut map, mmap);
    }
    map
}

fn enumerate_counts(
    counts: &mut [u8; 13],
    rank: usize,
    remaining: usize,
    map: &mut HashMap<u32, u32>,
    mmap: &Mmap,
) {
    if rank == 13 {
        if remaining == 0 {
            let key = rank_key(counts);
            map.entry(key)
                .or_insert_with(|| best_non_flush_rank(counts, mmap));
        }
        return;
    }
    let max_here = remaining.min(4);
    for k in 0..=max_here {
        counts[rank] = k as u8;
        enumerate_counts(counts, rank + 1, remaining - k, map, mmap);
    }
    counts[rank] = 0;
}

fn best_non_flush_rank(counts: &[u8; 13], mmap: &Mmap) -> u32 {
    let mut best = u32::MAX;
    let mut sel = [0u8; 5];
    choose_5(counts, 0, 0, &mut sel, &mut best, mmap);
    best
}

fn choose_5(
    counts: &[u8; 13],
    rank: usize,
    filled: usize,
    sel: &mut [u8; 5],
    best: &mut u32,
    mmap: &Mmap,
) {
    if filled == 5 {
        let r = eval_5_avoid_flush(sel, mmap);
        if r < *best {
            *best = r;
        }
        return;
    }
    if rank == 13 {
        return;
    }
    let max_here = (counts[rank] as usize).min(5 - filled);
    for k in 0..=max_here {
        for i in 0..k {
            sel[filled + i] = rank as u8;
        }
        choose_5(counts, rank + 1, filled + k, sel, best, mmap);
    }
}

/// Evaluate a 5-rank selection by assigning suit i%4 to card i. This
/// guarantees no flush (max 2 of any suit) so the LUT returns the
/// non-flush rank even for rank patterns that could otherwise form a
/// flush with different suit assignments.
#[inline]
fn eval_5_avoid_flush(sel: &[u8; 5], mmap: &Mmap) -> u32 {
    let mut cards = [0u8; 5];
    // Build cards under the crate's canonical encoding: suit * 13 + rank.
    // Suit assignment cycles through {0,1,2,3,0}; this guarantees at most
    // two cards share any suit, so the resulting hand cannot form a flush.
    for i in 0..5 {
        let suit = (i as u8) & 3;
        // Canonical encoding (matches slow.rs and hand_ranks.bin):
        // card = suit * 13 + rank.
        cards[i] = suit * 13 + sel[i];
    }
    cards.sort_unstable_by(|a, b| b.cmp(a));
    let idx = combinadic_rank(&cards) as usize;
    let offset = idx * 4;
    if offset + 4 > mmap.len() {
        return u32::MAX;
    }
    u32::from_le_bytes([
        mmap[offset],
        mmap[offset + 1],
        mmap[offset + 2],
        mmap[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NlheEvaluator;

    fn find_rank_table() -> Option<String> {
        // Resolve relative to the workspace root so the test runs from
        // any CWD (nextest uses the crate dir). Checks the current and
        // historical output locations.
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("crate is two levels deep");
        let candidates = [
            "outputs/v23/hand_ranks.bin",
            "outputs/v0-smoke/hand_ranks.bin",
            "outputs/v9/hand_ranks.bin",
            "outputs/v8/hand_ranks.bin",
        ];
        for cand in &candidates {
            let p = workspace_root.join(cand);
            if p.exists() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
        // Also try the manifest-relative location as a fallback.
        for cand in &candidates {
            if std::path::Path::new(cand).exists() {
                return Some((*cand).to_string());
            }
        }
        None
    }

    #[test]
    fn fast7_matches_slow_evaluator() {
        let table_path = match find_rank_table() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: no rank table on disk");
                return;
            }
        };
        let fast = Fast7Evaluator::new(&table_path).unwrap();
        let slow = NlheEvaluator;

        let mut seed: u64 = 0xDEAD_BEEF_CAFE_1234;
        let mut next = || -> u8 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) & 0x3F) as u8
        };

        for i in 0..50_000 {
            let mut used = [false; 52];
            let mut cards = [0u8; 7];
            let mut n = 0;
            while n < 7 {
                let c = (next() % 52) as usize;
                if !used[c] {
                    used[c] = true;
                    cards[n] = c as u8;
                    n += 1;
                }
            }
            let hole = [cards[0], cards[1]];
            let board = [cards[2], cards[3], cards[4], cards[5], cards[6]];
            let f = fast.evaluate_hand(&hole, &board);
            let s = slow.evaluate_hand(&hole, &board);
            assert_eq!(f, s, "mismatch at iteration {i}: cards {:?}", cards);
        }
    }

    #[test]
    fn fast7_matches_table_evaluator() {
        let table_path = match find_rank_table() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: no rank table on disk");
                return;
            }
        };
        let fast = Fast7Evaluator::new(&table_path).unwrap();
        let table = crate::TableEvaluator::new(&table_path).unwrap();

        let mut seed: u64 = 0x1234_5678_9ABC_DEF0;
        let mut next = || -> u8 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) & 0x3F) as u8
        };

        for i in 0..50_000 {
            let mut used = [false; 52];
            let mut cards = [0u8; 7];
            let mut n = 0;
            while n < 7 {
                let c = (next() % 52) as usize;
                if !used[c] {
                    used[c] = true;
                    cards[n] = c as u8;
                    n += 1;
                }
            }
            let hole = [cards[0], cards[1]];
            let board = [cards[2], cards[3], cards[4], cards[5], cards[6]];
            let a = fast.evaluate_hand(&hole, &board);
            let b = table.evaluate_hand(&hole, &board);
            assert_eq!(a, b, "mismatch at iteration {i}: cards {:?}", cards);
        }
    }

    #[test]
    fn non_flush_lut_is_populated() {
        let table_path = match find_rank_table() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: no rank table on disk");
                return;
            }
        };
        let fast = Fast7Evaluator::new(&table_path).unwrap();
        let n = fast.non_flush_len();
        assert!(n > 10_000, "LUT has only {n} entries - expected >10K");
        assert!(n < 100_000, "LUT has {n} entries - suspiciously large");
    }
}
