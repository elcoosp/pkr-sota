#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here

pub mod ehs;
pub use ehs::calculate_ehs;

use memmap2::Mmap;
use pkr_contracts::{fnv1a, AbstractionBuilder, Evaluator, FNV_OFFSET};
use pkr_eval::lookup::{choose, combinadic_rank};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentroidStore {
    pub centroids: Vec<(f32, f32)>,
}

pub fn load_centroids(path: &str) -> Result<CentroidStore, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let store: CentroidStore = bincode::deserialize_from(reader)?;
    Ok(store)
}

pub fn save_centroids(path: &str, store: &CentroidStore) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    bincode::serialize_into(file, store)?;
    Ok(())
}

pub struct KMeansAbstraction {
    centroids: HashMap<u8, Vec<(f32, f32)>>,
    default_centroids: Vec<(f32, f32)>,
    tables: HashMap<u8, OnceLock<Mmap>>,
    flop_buckets: OnceLock<Vec<u8>>,
    evaluator: Arc<dyn Evaluator>,
}

impl KMeansAbstraction {
    pub fn new(default_centroids: Vec<(f32, f32)>, evaluator: Arc<dyn Evaluator>) -> Self {
        let mut tables = HashMap::new();
        for s in 0u8..=3 {
            tables.insert(s, OnceLock::new());
        }
        KMeansAbstraction {
            centroids: HashMap::new(),
            default_centroids,
            tables,
            flop_buckets: OnceLock::new(),
            evaluator,
        }
    }

    pub fn from_store(store: CentroidStore, evaluator: Arc<dyn Evaluator>) -> Self {
        Self::new(store.centroids, evaluator)
    }

    pub fn load_street_centroids(
        &mut self,
        street_code: u8,
        path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let store = load_centroids(path)?;
        self.centroids.insert(street_code, store.centroids);
        Ok(())
    }

    pub fn init_table(&self, street_code: u8, path: &str) -> Result<(), std::io::Error> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let lock = self
            .tables
            .get(&street_code)
            .expect("table slot not created");
        lock.set(mmap).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::AlreadyExists, "table already set")
        })?;
        Ok(())
    }

    pub fn load_flop_buckets(&self, path: &str) -> Result<(), std::io::Error> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        self.flop_buckets.set(data).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "flop buckets already set",
            )
        })?;
        Ok(())
    }

    fn flop_bucket(&self, board: &[u8]) -> u8 {
        if board.len() >= 3 {
            if let Some(buckets) = self.flop_buckets.get() {
                let mut flop = [board[0], board[1], board[2]];
                flop.sort_unstable_by(|a, b| b.cmp(a));
                let idx = choose(flop[0] as u32, 3) as usize
                    + choose(flop[1] as u32, 2) as usize
                    + choose(flop[2] as u32, 1) as usize;
                if idx < buckets.len() {
                    return buckets[idx];
                }
            }
        }
        0
    }

    fn flat_index_preflop(hole: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_ne!(hole[0], hole[1]);
        let mut cards = [hole[0], hole[1]];
        cards.sort_unstable_by(|a, b| b.cmp(a));
        choose(cards[0] as u32, 2) as usize + choose(cards[1] as u32, 1) as usize
    }

    fn flat_index_flop(hole: &[u8], board: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_eq!(board.len(), 3);
        assert_ne!(hole[0], hole[1]);
        let mut all = [0u8; 5];
        all[0] = hole[0];
        all[1] = hole[1];
        all[2] = board[0];
        all[3] = board[1];
        all[4] = board[2];
        all.sort_unstable_by(|a, b| b.cmp(a));
        let combo_idx = combinadic_rank(&all) as usize;
        let hole_set = [hole[0], hole[1]];
        let masks: [[usize; 2]; 10] = [
            [0, 1],
            [0, 2],
            [0, 3],
            [0, 4],
            [1, 2],
            [1, 3],
            [1, 4],
            [2, 3],
            [2, 4],
            [3, 4],
        ];
        let mut mask_idx = 0;
        for (mi, pos) in masks.iter().enumerate() {
            let h1 = all[pos[0]];
            let h2 = all[pos[1]];
            if hole_set.contains(&h1) && hole_set.contains(&h2) {
                mask_idx = mi;
                break;
            }
        }
        combo_idx * 10 + mask_idx
    }

    fn flat_index_turn(hole: &[u8], board: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_eq!(board.len(), 4);
        let mut all = [0u8; 6];
        all[0] = hole[0];
        all[1] = hole[1];
        all[2] = board[0];
        all[3] = board[1];
        all[4] = board[2];
        all[5] = board[3];
        all.sort_unstable_by(|a, b| b.cmp(a));
        let rank = combinadic_rank_6(&all);
        let hole_set = [hole[0], hole[1]];
        let masks: [[usize; 2]; 15] = [
            [0, 1],
            [0, 2],
            [0, 3],
            [0, 4],
            [0, 5],
            [1, 2],
            [1, 3],
            [1, 4],
            [1, 5],
            [2, 3],
            [2, 4],
            [2, 5],
            [3, 4],
            [3, 5],
            [4, 5],
        ];
        let mut mask_idx = 0;
        for (mi, pos) in masks.iter().enumerate() {
            let h1 = all[pos[0]];
            let h2 = all[pos[1]];
            if hole_set.contains(&h1) && hole_set.contains(&h2) {
                mask_idx = mi;
                break;
            }
        }
        (rank as usize) * 15 + mask_idx
    }

    fn flat_index_river_board(board: &[u8]) -> usize {
        debug_assert_eq!(board.len(), 5);
        let mut sorted = [board[0], board[1], board[2], board[3], board[4]];
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        combinadic_rank(&sorted) as usize
    }
}

fn combinadic_rank_6(cards: &[u8; 6]) -> u64 {
    let c0 = cards[0] as u32;
    let c1 = cards[1] as u32;
    let c2 = cards[2] as u32;
    let c3 = cards[3] as u32;
    let c4 = cards[4] as u32;
    let c5 = cards[5] as u32;
    choose(c0, 6) as u64
        + choose(c1, 5) as u64
        + choose(c2, 4) as u64
        + choose(c3, 3) as u64
        + choose(c4, 2) as u64
        + choose(c5, 1) as u64
}

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64 {
        let centroids = self
            .centroids
            .get(&street)
            .unwrap_or(&self.default_centroids);

        let ehs_fallback = || {
            warn_mc_fallback_once();
            let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
            nearest_centroid(ehs, ehs_sq, centroids)
        };
        let cluster_id = match board.len() {
            0 => {
                if let Some(table) = self.tables.get(&0u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_preflop(hole);
                    if idx < table.len() {
                        table[idx] as u64
                    } else {
                        ehs_fallback()
                    }
                } else {
                    ehs_fallback()
                }
            }
            3 => {
                if let Some(table) = self.tables.get(&1u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_flop(hole, board);
                    if idx < table.len() {
                        table[idx] as u64
                    } else {
                        ehs_fallback()
                    }
                } else {
                    ehs_fallback()
                }
            }
            4 => {
                if let Some(table) = self.tables.get(&2u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_turn(hole, board);
                    if idx < table.len() {
                        table[idx] as u64
                    } else {
                        ehs_fallback()
                    }
                } else {
                    ehs_fallback()
                }
            }
            5 => {
                // River: bucket hand rank into ~128 tiers. Raw hand_rank
                // has cardinality 7462, which alone produces millions of
                // river infosets over a full training run and dominates the
                // map size. >> 6 gives 116 tiers — coarse enough to make
                // CFR see each river infoset repeatedly, fine enough to
                // preserve strategic distinctions (a made hand vs a busted
                // draw vs a middle pair still land in different tiers).
                // The precomputed board bucket (if any) is mixed in.
                let hand_rank = self.evaluator.evaluate_hand(hole, board) as u64;
                let hand_bucket = hand_rank >> 6;
                let board_bucket = if let Some(table) = self.tables.get(&3u8).and_then(|l| l.get())
                {
                    let idx = Self::flat_index_river_board(board);
                    if idx < table.len() {
                        table[idx] as u64
                    } else {
                        0
                    }
                } else {
                    0
                };
                (hand_bucket << 8) | (board_bucket & 0xff)
            }
            _ => ehs_fallback(),
        };

        let flop_bucket = self.flop_bucket(board);

        let mut h: u64 = FNV_OFFSET;
        fnv1a(&mut h, std::slice::from_ref(&street));
        fnv1a(&mut h, &[history.len() as u8]);
        fnv1a(&mut h, history);
        fnv1a(&mut h, &cluster_id.to_le_bytes());
        fnv1a(&mut h, &[flop_bucket]);
        h
    }
}

fn warn_mc_fallback_once() {
    use std::sync::OnceLock;
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!(
            "WARNING: abstraction fell back to Monte-Carlo EHS. This is \
             ~100x slower per infoset than the precomputed table path. \
             Likely cause: --preflop-table / --flop-table / --flop-buckets \
             not loaded, or an index fell outside the table's range."
        );
    });
}

fn nearest_centroid(ehs: f32, ehs_sq: f32, centroids: &[(f32, f32)]) -> u64 {
    centroids
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let c1 = a.1;
            let c2 = b.1;
            let dx1 = ehs - c1.0;
            let dy1 = ehs_sq - c1.1;
            let dx2 = ehs - c2.0;
            let dy2 = ehs_sq - c2.1;
            (dx1 * dx1 + dy1 * dy1).total_cmp(&(dx2 * dx2 + dy2 * dy2))
        })
        .map(|(idx, _)| idx as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::Evaluator;
    struct MockEvaluator;
    impl Evaluator for MockEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 {
            0u32
        }
    }

    #[test]
    fn test_history_street_hash_golden() {
        // GOLDEN VECTORS — these constants are part of the on-disk format contract.
        // If these values change, every exported blueprint in existence is invalidated
        // (requires a format_version bump + full retrain + re-export).
        //
        // Computed with FNV-1a 64-bit, little-endian cluster_id, length-prefixed history.
        let builder =
            KMeansAbstraction::new(vec![(0.3, 0.09), (0.7, 0.49)], Arc::new(MockEvaluator));
        assert_eq!(
            builder.get_infoset_hash(&[0, 1], &[], &[], 0),
            0xc885ccdc03990c97
        );
        assert_eq!(
            builder.get_infoset_hash(&[0, 1], &[], &[0], 0),
            0x23ae4ee1fbe6228a
        );
        assert_ne!(
            builder.get_infoset_hash(&[0, 1], &[], &[], 0),
            builder.get_infoset_hash(&[0, 1], &[], &[0], 0),
            "history must be part of the hash"
        );
    }

    #[test]
    fn test_history_street_hash() {
        let builder =
            KMeansAbstraction::new(vec![(0.3, 0.09), (0.7, 0.49)], Arc::new(MockEvaluator));
        let h1 = builder.get_infoset_hash(&[0, 1], &[], &[], 0);
        let h2 = builder.get_infoset_hash(&[0, 1], &[], &[0], 0);
        assert_ne!(h1, h2);
    }
}
#[cfg(test)]
mod extended_tests {
    use super::*;
    use pkr_contracts::Evaluator;
    use std::sync::Arc;
    struct TestEval;
    impl Evaluator for TestEval {
        fn evaluate_hand(&self, _: &[u8], _: &[u8]) -> u32 {
            0
        }
    }

    #[test]
    fn test_flat_index_preflop_boundaries() {
        assert_eq!(KMeansAbstraction::flat_index_preflop(&[0, 1]), 0);
        assert_eq!(KMeansAbstraction::flat_index_preflop(&[50, 51]), 1325);
    }

    #[test]
    fn test_flat_index_flop_consistency() {
        let idx1 = KMeansAbstraction::flat_index_flop(&[10, 20], &[30, 40, 50]);
        let idx2 = KMeansAbstraction::flat_index_flop(&[20, 10], &[50, 30, 40]);
        assert_eq!(idx1, idx2);
    }

    #[test]
    fn test_flat_index_turn_consistency() {
        let idx1 = KMeansAbstraction::flat_index_turn(&[5, 15], &[25, 35, 45, 51]);
        let idx2 = KMeansAbstraction::flat_index_turn(&[15, 5], &[51, 35, 25, 45]);
        assert_eq!(idx1, idx2);
    }

    #[test]
    fn test_centroid_save_and_load() {
        let store = CentroidStore {
            centroids: vec![(0.1, 0.01), (0.5, 0.25), (0.9, 0.81)],
        };
        let tmp = std::env::temp_dir().join("test_centroids.bin");
        save_centroids(tmp.to_str().unwrap(), &store).unwrap();
        let loaded = load_centroids(tmp.to_str().unwrap()).unwrap();
        assert_eq!(loaded.centroids.len(), 3);
        assert!((loaded.centroids[1].0 - 0.5).abs() < 0.001);
        std::fs::remove_file(tmp).ok();
    }

    #[test]
    fn test_river_centroids_support_large_k() {
        // River centroids should support k up to 2000 (u16 range)
        let n: usize = 1500;
        let store = CentroidStore {
            centroids: (0..n)
                .map(|i| (i as f32 / n as f32, i as f32 / n as f32))
                .collect(),
        };
        let tmp = std::env::temp_dir().join("test_river_centroids.bin");
        save_centroids(tmp.to_str().unwrap(), &store).unwrap();
        let loaded = load_centroids(tmp.to_str().unwrap()).unwrap();
        assert_eq!(loaded.centroids.len(), n);
        std::fs::remove_file(tmp).ok();
    }

    #[test]
    fn test_hash_changes_with_street() {
        let builder = KMeansAbstraction::new(vec![(0.5, 0.25)], Arc::new(TestEval));
        let h1 = builder.get_infoset_hash(&[0, 1], &[], &[], 0);
        let h2 = builder.get_infoset_hash(&[0, 1], &[2, 3, 4], &[], 1);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_changes_with_history_length() {
        let builder = KMeansAbstraction::new(vec![(0.5, 0.25)], Arc::new(TestEval));
        let h1 = builder.get_infoset_hash(&[0, 1], &[2, 3, 4], &[0], 1);
        let h2 = builder.get_infoset_hash(&[0, 1], &[2, 3, 4], &[0, 1], 1);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_flop_bucket_default_zero() {
        let builder = KMeansAbstraction::new(vec![(0.5, 0.25)], Arc::new(TestEval));
        let h = builder.get_infoset_hash(&[0, 1], &[2, 3, 4], &[], 1);
        assert!(h != 0);
    }

    #[test]
    fn test_abstraction_is_thread_safe() {
        use std::thread;
        let builder = Arc::new(KMeansAbstraction::new(
            vec![(0.3, 0.09), (0.7, 0.49)],
            Arc::new(TestEval),
        ));
        let mut handles = vec![];
        for _ in 0..4 {
            let b = Arc::clone(&builder);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    assert!(b.get_infoset_hash(&[0, 1], &[2, 3, 4], &[0, 1], 1) != 0);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }
}
