use pkr_contracts::Evaluator;
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

/// Binomial coefficient for combinadic indexing.
pub fn choose(n: u32, k: u32) -> u32 {
    if k > n {
        return 0;
    }
    match k {
        0 => 1,
        1 => n,
        2 => n * (n - 1) / 2,
        3 => n * (n - 1) * (n - 2) / 6,
        4 => n * (n - 1) * (n - 2) * (n - 3) / 24,
        5 => n * (n - 1) * (n - 2) * (n - 3) * (n - 4) / 120,
        _ => panic!("k > 5 not supported"),
    }
}

/// Combinadic rank of a 5-card combination sorted descending.
pub fn combinadic_rank(cards: &[u8; 5]) -> u32 {
    let c0 = cards[0] as u32;
    let c1 = cards[1] as u32;
    let c2 = cards[2] as u32;
    let c3 = cards[3] as u32;
    let c4 = cards[4] as u32;
    choose(c0, 1) + choose(c1, 2) + choose(c2, 3) + choose(c3, 4) + choose(c4, 5)
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
