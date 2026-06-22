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

/// Unrank combinadic index to k-card combination (descending).
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

    fn eval_5_fast(&self, hand: &[u8; 5]) -> u32 {
        let mut sorted = *hand;
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        let idx = combinadic_rank(&sorted) as usize;
        let offset = idx * 4;
        let bytes = &self.mmap[offset..offset + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }
}

impl Evaluator for TableEvaluator {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
        let total = hole.len() + board.len();
        assert!(total >= 5 && total <= 7);
        let mut cards = [255u8; 7];
        let mut idx = 0;
        for &c in hole.iter().chain(board) {
            cards[idx] = c;
            idx += 1;
        }
        let mut best = u32::MAX;
        const COMBOS_7_5: [[u8; 5]; 21] = [
            [0,1,2,3,4], [0,1,2,3,5], [0,1,2,3,6],
            [0,1,2,4,5], [0,1,2,4,6], [0,1,2,5,6],
            [0,1,3,4,5], [0,1,3,4,6], [0,1,3,5,6],
            [0,1,4,5,6], [0,2,3,4,5], [0,2,3,4,6],
            [0,2,3,5,6], [0,2,4,5,6], [0,3,4,5,6],
            [1,2,3,4,5], [1,2,3,4,6], [1,2,3,5,6],
            [1,2,4,5,6], [1,3,4,5,6], [2,3,4,5,6],
        ];
        let num = match total { 5=>1, 6=>6, 7=>21, _=>0 };
        for i in 0..num {
            let combo = COMBOS_7_5[i];
            let mut hand = [0u8; 5];
            for (j, &ci) in combo.iter().enumerate() {
                hand[j] = cards[ci as usize];
            }
            let r = self.eval_5_fast(&hand);
            if r < best { best = r; }
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
        assert_eq!(choose(6, 3), 20);
        assert_eq!(choose(50, 7), 99_884_400);
        assert_eq!(choose(51, 7), 115_775_100);
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
