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

    #[inline(always)]
    fn eval_5_fast(&self, hand: &[u8; 5]) -> u32 {
        let mut sorted = *hand;
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        let idx = combinadic_rank(&sorted) as usize;
        let max_idx = self.mmap.len() / 4;
        if idx >= max_idx {
            eprintln!("WARNING: invalid hand rank {} (max {})", idx, max_idx);
            return u32::MAX;
        }
        let offset = idx * 4;
        let bytes = &self.mmap[offset..offset + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
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
            best = self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[3], cards[4]]);
        } else if total == 6 {
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[3], cards[4]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[3], cards[5]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[4], cards[5]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[3], cards[4], cards[5]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[2], cards[3], cards[4], cards[5]]));
            best = best.min(self.eval_5_fast(&[cards[1], cards[2], cards[3], cards[4], cards[5]]));
        } else if total == 7 {
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[3], cards[4]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[3], cards[5]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[3], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[4], cards[5]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[4], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[2], cards[5], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[3], cards[4], cards[5]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[3], cards[4], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[3], cards[5], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[1], cards[4], cards[5], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[2], cards[3], cards[4], cards[5]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[2], cards[3], cards[4], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[2], cards[3], cards[5], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[2], cards[4], cards[5], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[0], cards[3], cards[4], cards[5], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[1], cards[2], cards[3], cards[4], cards[5]]));
            best = best.min(self.eval_5_fast(&[cards[1], cards[2], cards[3], cards[4], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[1], cards[2], cards[3], cards[5], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[1], cards[2], cards[4], cards[5], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[1], cards[3], cards[4], cards[5], cards[6]]));
            best = best.min(self.eval_5_fast(&[cards[2], cards[3], cards[4], cards[5], cards[6]]));
        }
        best
    }
}
