use super::lookup::{choose, combinadic_rank};
use memmap2::Mmap;
use pkr_contracts::Evaluator;
use std::fs::File;
use std::path::Path;

pub fn combinadic_unrank_2(mut index: u32) -> [u8; 2] {
    let mut result = [0u8; 2];
    let mut remaining = 52u32;
    for i in (1..=2).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index {
            x -= 1;
        }
        let pos = (2 - i) as usize;
        result[pos] = x as u8;
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

pub fn combinadic_unrank_3(mut index: u32) -> [u8; 3] {
    let mut result = [0u8; 3];
    let mut remaining = 52u32;
    for i in (1..=3).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index {
            x -= 1;
        }
        let pos = (3 - i) as usize;
        result[pos] = x as u8;
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

pub fn combinadic_unrank_5(mut index: u32) -> [u8; 5] {
    let mut result = [0u8; 5];
    let mut remaining = 52u32;
    for i in (1..=5).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index {
            x -= 1;
        }
        let pos = (5 - i) as usize;
        result[pos] = x as u8;
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

pub fn combinadic_unrank_6(mut index: u32) -> [u8; 6] {
    let mut result = [0u8; 6];
    let mut remaining = 52u32;
    for i in (1..=6).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index {
            x -= 1;
        }
        let pos = (6 - i) as usize;
        result[pos] = x as u8;
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

pub fn combinadic_unrank_7(mut index: u32) -> [u8; 7] {
    let mut result = [0u8; 7];
    let mut remaining = 52u32;
    for i in (1..=7).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index {
            x -= 1;
        }
        let pos = (7 - i) as usize;
        result[pos] = x as u8;
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

pub fn combinadic_unrank(mut index: u32, k: u32, n: u32) -> Vec<u8> {
    let mut result = Vec::with_capacity(k as usize);
    let mut remaining = n;
    for i in (1..=k).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index {
            x -= 1;
        }
        result.push(x as u8);
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

pub struct TableEvaluator {
    mmap: Mmap,
}

impl TableEvaluator {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        Ok(TableEvaluator { mmap })
    }

    /// Direct combinadic LUT read. Caller MUST pass cards sorted
    /// descending. This is the T1.3 hot path: no sort, no allocation.
    #[inline(always)]
    fn load_rank_sorted(&self, sorted: &[u8; 5]) -> u32 {
        let idx = combinadic_rank(sorted) as usize;
        let max_idx = self.mmap.len() / 4;
        if idx >= max_idx {
            eprintln!("WARNING: invalid hand rank {} (max {})", idx, max_idx);
            return u32::MAX;
        }
        let offset = idx * 4;
        let bytes = &self.mmap[offset..offset + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Sorts once then reads. Retained for tests and any non-hot caller.
    #[allow(dead_code)]
    #[inline]
    fn eval_5_fast(&self, hand: &[u8; 5]) -> u32 {
        let mut sorted = *hand;
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        self.load_rank_sorted(&sorted)
    }
}

impl Evaluator for TableEvaluator {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
        let mut cards = [0u8; 7];
        let mut total = 0;

        // Filter out sentinel values (≥52) AND duplicate cards
        for &c in hole.iter().chain(board) {
            if c < 52 && !cards[..total].contains(&c) {
                cards[total] = c;
                total += 1;
            }
        }

        if total < 5 {
            return u32::MAX;
        }

        cards[..total].sort_unstable_by(|a, b| b.cmp(a)); // sort once, descending

        let mut best = u32::MAX;
        if total == 5 {
            best = self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[3], cards[4]]);
        } else if total == 6 {
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[3], cards[4]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[3], cards[5]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[4], cards[5]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[3], cards[4], cards[5]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[2], cards[3], cards[4], cards[5]]));
            best = best
                .min(self.load_rank_sorted(&[cards[1], cards[2], cards[3], cards[4], cards[5]]));
        } else if total == 7 {
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[3], cards[4]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[3], cards[5]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[3], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[4], cards[5]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[4], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[2], cards[5], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[3], cards[4], cards[5]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[3], cards[4], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[3], cards[5], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[1], cards[4], cards[5], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[2], cards[3], cards[4], cards[5]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[2], cards[3], cards[4], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[2], cards[3], cards[5], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[2], cards[4], cards[5], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[0], cards[3], cards[4], cards[5], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[1], cards[2], cards[3], cards[4], cards[5]]));
            best = best
                .min(self.load_rank_sorted(&[cards[1], cards[2], cards[3], cards[4], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[1], cards[2], cards[3], cards[5], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[1], cards[2], cards[4], cards[5], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[1], cards[3], cards[4], cards[5], cards[6]]));
            best = best
                .min(self.load_rank_sorted(&[cards[2], cards[3], cards[4], cards[5], cards[6]]));
        }
        best
    }
}

#[cfg(test)]
mod t13_tests {
    use super::*;
    use crate::NlheEvaluator;
    use pkr_contracts::Evaluator;

    fn find_rank_table() -> Option<String> {
        // Resolve against the workspace root (nextest runs from the crate dir).
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
        let fast = TableEvaluator::new(&table_path).unwrap();
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
            assert_eq!(f, s, "mismatch iteration {i}: cards {:?}", cards);
        }
    }

    #[test]
    #[ignore = "requires generated hand_ranks.bin; run after regenerating the table"]
    fn table_two_pair_ordering_matches_slow_eval() {
        let path = std::env::var("PKR_RANK_TABLE").unwrap_or_else(|_| "hand_ranks.bin".into());
        let t = TableEvaluator::new(&path).unwrap();
        let kk447: [u8; 5] = [11, 24, 2, 15, 5];
        let qq229: [u8; 5] = [10, 23, 0, 13, 7];
        let a = t.evaluate_hand(&kk447, &[]);
        let b = t.evaluate_hand(&qq229, &[]);
        assert!(
            a < b,
            "table must rank KK447 better than QQ229 (lower = better)"
        );
    }

    #[test]
    #[ignore]
    fn fast7_microbench() {
        let table_path = match find_rank_table() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: no rank table on disk");
                return;
            }
        };
        let fast = TableEvaluator::new(&table_path).unwrap();
        let slow = NlheEvaluator;

        let mut seed: u64 = 42;
        let mut next = || -> u8 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) & 0x3F) as u8
        };

        let mut hands: Vec<([u8; 2], [u8; 5])> = Vec::with_capacity(100_000);
        for _ in 0..100_000 {
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
            hands.push((
                [cards[0], cards[1]],
                [cards[2], cards[3], cards[4], cards[5], cards[6]],
            ));
        }

        let t0 = std::time::Instant::now();
        let mut acc: u64 = 0;
        for (h, b) in &hands {
            acc = acc.wrapping_add(fast.evaluate_hand(h, b) as u64);
        }
        let fast_t = t0.elapsed();

        let t1 = std::time::Instant::now();
        let mut acc2: u64 = 0;
        for (h, b) in &hands {
            acc2 = acc2.wrapping_add(slow.evaluate_hand(h, b) as u64);
        }
        let slow_t = t1.elapsed();

        assert_eq!(acc, acc2);
        let ratio = slow_t.as_secs_f64() / fast_t.as_secs_f64();
        eprintln!(
            "fast7:  {:.0} ns/eval",
            fast_t.as_secs_f64() * 1e9 / hands.len() as f64
        );
        eprintln!(
            "slow:   {:.0} ns/eval",
            slow_t.as_secs_f64() * 1e9 / hands.len() as f64
        );
        eprintln!("ratio:  {:.2}x", ratio);
    }
}
