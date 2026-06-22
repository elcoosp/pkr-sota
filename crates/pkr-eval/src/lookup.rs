use pkr_contracts::Evaluator;
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

/// Binomial coefficient C(n,k), safe for 0≤k≤7. Uses u64 for intermediates.
pub fn choose(n: u32, k: u32) -> u32 {
    if k > n { return 0; }
    let n = n as u64;
    let result: u64 = match k {
        0 => 1,
        1 => n,
        2 => n * (n - 1) / 2,
        3 => n * (n - 1) * (n - 2) / 6,
        4 => n * (n - 1) * (n - 2) * (n - 3) / 24,
        5 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) / 120,
        6 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) * (n - 5) / 720,
        7 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) * (n - 5) * (n - 6) / 5040,
        _ => panic!("k>7 unsupported"),
    };
    result as u32
}

/// Combinadic rank of a 5-card combination sorted descending.
pub fn combinadic_rank(cards: &[u8; 5]) -> u32 {
    let c0 = cards[0] as u32;
    let c1 = cards[1] as u32;
    let c2 = cards[2] as u32;
    let c3 = cards[3] as u32;
    let c4 = cards[4] as u32;
    choose(c0, 5) + choose(c1, 4) + choose(c2, 3) + choose(c3, 2) + choose(c4, 1)
}

/// Unrank combinadic index to 2-card combination (descending).
pub fn combinadic_unrank_2(mut index: u32) -> [u8; 2] {
    let mut result = [0u8; 2];
    let mut remaining = 52u32;
    for i in (1..=2).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index { x -= 1; }
        let pos = (2 - i) as usize;
        result[pos] = x as u8;
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

/// Unrank combinadic index to 5-card combination (descending).
pub fn combinadic_unrank_5(mut index: u32) -> [u8; 5] {
    let mut result = [0u8; 5];
    let mut remaining = 52u32;
    for i in (1..=5).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index { x -= 1; }
        let pos = (5 - i) as usize;
        result[pos] = x as u8;
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

/// Unrank combinadic index to 6-card combination (descending).
pub fn combinadic_unrank_6(mut index: u32) -> [u8; 6] {
    let mut result = [0u8; 6];
    let mut remaining = 52u32;
    for i in (1..=6).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index { x -= 1; }
        let pos = (6 - i) as usize;
        result[pos] = x as u8;
        index -= choose(x, i);
        remaining = x;
    }
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
}

/// Unrank combinadic index to k-card combination (descending) as a Vec (for generic use).
pub fn combinadic_unrank(mut index: u32, k: u32, n: u32) -> Vec<u8> {
    let mut result = Vec::with_capacity(k as usize);
    let mut remaining = n;
    for i in (1..=k).rev() {
        let mut x = remaining - 1;
        while choose(x, i) > index { x -= 1; }
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
        let idx = combinadic_rank(hand) as usize;
        let offset = idx * 4;
        let bytes = &self.mmap[offset..offset + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

impl Evaluator for TableEvaluator {
    #[inline]
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
        let total = hole.len() + board.len();
        let mut cards = [0u8; 7];
        cards[..hole.len()].copy_from_slice(hole);
        cards[hole.len()..total].copy_from_slice(board);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_6_7() {
        assert_eq!(choose(52, 6), 20_358_520);
        assert_eq!(choose(52, 7), 133_784_560);
    }

    #[test]
    fn combinadic_roundtrip_5() {
        assert_eq!(combinadic_rank(&[4,3,2,1,0]), 0);
        assert_eq!(combinadic_rank(&[51,50,49,48,47]), 2_598_959);
    }

    #[test]
    fn combinadic_roundtrip_full() {
        let mut rng = rand::rng();
        for _ in 0..1000 {
            let mut cards: Vec<u8> = (0..52).collect();
            use rand::seq::SliceRandom;
            cards.shuffle(&mut rng);
            let mut hand = [0u8; 5];
            hand.copy_from_slice(&cards[..5]);
            hand.sort_unstable_by(|a, b| b.cmp(a));
            let rank = combinadic_rank(&hand);
            let rebuilt = combinadic_unrank_5(rank);
            assert_eq!(hand, rebuilt, "roundtrip failed for rank {}", rank);
        }
    }
}
