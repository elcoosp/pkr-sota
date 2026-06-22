pub mod cluster;
pub mod ehs;
pub use ehs::calculate_ehs;

use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_eval::lookup::combinadic_rank;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, OnceLock};
use memmap2::Mmap;

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
    /// Precomputed tables for streets (flop=1, turn=2, river=3). Initialized via init_table.
    tables: HashMap<u8, OnceLock<Mmap>>,
    evaluator: Arc<dyn Evaluator>,
}

impl KMeansAbstraction {
    pub fn new(
        default_centroids: Vec<(f32, f32)>,
        evaluator: Arc<dyn Evaluator>,
    ) -> Self {
        let mut tables = HashMap::new();
        // Reserve entries for streets 1,2,3
        for s in [1u8,2,3] {
            tables.insert(s, OnceLock::new());
        }
        KMeansAbstraction {
            centroids: HashMap::new(),
            default_centroids,
            tables,
            evaluator,
        }
    }

    pub fn from_store(store: CentroidStore, evaluator: Arc<dyn Evaluator>) -> Self {
        Self::new(store.centroids, evaluator)
    }

    pub fn load_street_centroids(&mut self, street_code: u8, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let store = load_centroids(path)?;
        self.centroids.insert(street_code, store.centroids);
        Ok(())
    }

    /// Load a precomputed table for a street (1=flop,2=turn,3=river). Thread-safe.
    pub fn init_table(&self, street_code: u8, path: &str) -> Result<(), std::io::Error> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let lock = self.tables.get(&street_code)
            .expect("table slot not created");
        lock.set(mmap).map_err(|_| std::io::Error::new(std::io::ErrorKind::AlreadyExists, "table already set"))?;
        Ok(())
    }

    /// Index into the flop lookup table.
    fn flat_index_flop(hole: &[u8], board: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_eq!(board.len(), 3);
        let mut all = [0u8; 5];
        all[0] = hole[0]; all[1] = hole[1];
        all[2] = board[0]; all[3] = board[1]; all[4] = board[2];
        all.sort_unstable_by(|a, b| b.cmp(a));
        let combo_idx = combinadic_rank(&all) as usize;
        let hole_set = [hole[0], hole[1]];
        let masks: [[usize; 2]; 10] = [
            [0,1],[0,2],[0,3],[0,4],
            [1,2],[1,3],[1,4],[2,3],[2,4],[3,4],
        ];
        let mut mask_idx = 0;
        for (mi, pos) in masks.iter().enumerate() {
            let h1 = all[pos[0]]; let h2 = all[pos[1]];
            if hole_set.contains(&h1) && hole_set.contains(&h2) {
                mask_idx = mi; break;
            }
        }
        combo_idx * 10 + mask_idx
    }

    /// Index into turn lookup table (6 cards: 2 hole + 4 board). Similar to flop but with 6 cards.
    fn flat_index_turn(hole: &[u8], board: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_eq!(board.len(), 4);
        let mut all = [0u8; 6];
        all[0] = hole[0]; all[1] = hole[1];
        all[2] = board[0]; all[3] = board[1]; all[4] = board[2]; all[5] = board[3];
        all.sort_unstable_by(|a, b| b.cmp(a));
        // combinadic rank for 6 cards: compute choose() sums
        let rank = choose_6(all[0] as u32, all[1] as u32, all[2] as u32, all[3] as u32, all[4] as u32, all[5] as u32);
        let hole_set = [hole[0], hole[1]];
        // 6-choose-2 = 15 masks
        let masks: [[usize; 2]; 15] = [
            [0,1],[0,2],[0,3],[0,4],[0,5],
            [1,2],[1,3],[1,4],[1,5],
            [2,3],[2,4],[2,5],
            [3,4],[3,5],
            [4,5],
        ];
        let mut mask_idx = 0;
        for (mi, pos) in masks.iter().enumerate() {
            let h1 = all[pos[0]]; let h2 = all[pos[1]];
            if hole_set.contains(&h1) && hole_set.contains(&h2) {
                mask_idx = mi; break;
            }
        }
        (rank as usize) * 15 + mask_idx
    }
}

// Combinadic rank for 6 cards (descending)
fn choose_6(a0: u32, a1: u32, a2: u32, a3: u32, a4: u32, a5: u32) -> u64 {
    use pkr_eval::lookup::choose;
    choose(a0, 6) as u64 + choose(a1, 5) as u64 + choose(a2, 4) as u64 + choose(a3, 3) as u64 + choose(a4, 2) as u64 + choose(a5, 1) as u64
}

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64 {
        let centroids = self.centroids.get(&street)
            .unwrap_or(&self.default_centroids);

        let cluster_id = match board.len() {
            3 => {
                if let Some(table) = self.tables.get(&1u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_flop(hole, board);
                    table[idx] as u64
                } else {
                    // MC fallback
                    let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
                    nearest_centroid(ehs, ehs_sq, centroids)
                }
            },
            4 => {
                if let Some(table) = self.tables.get(&2u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_turn(hole, board);
                    table[idx] as u64
                } else {
                    let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
                    nearest_centroid(ehs, ehs_sq, centroids)
                }
            },
            5 => {
                if let Some(table) = self.tables.get(&3u8).and_then(|l| l.get()) {
                    // River table index: 7 cards total (2+5). We'll need a separate function, but for now fallback.
                    let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
                    nearest_centroid(ehs, ehs_sq, centroids)
                } else {
                    let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
                    nearest_centroid(ehs, ehs_sq, centroids)
                }
            },
            _ => {
                let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
                nearest_centroid(ehs, ehs_sq, centroids)
            }
        };

        let mut h: u64 = 0x9E3779B97F4A7C15;
        h ^= street as u64;
        h = h.wrapping_mul(31);
        for &b in history {
            h ^= b as u64;
            h = h.wrapping_mul(31);
        }
        h ^= cluster_id;
        h
    }
}

fn nearest_centroid(ehs: f32, ehs_sq: f32, centroids: &[(f32, f32)]) -> u64 {
    centroids.iter()
        .enumerate()
        .min_by(|a, b| {
            let c1 = a.1; let c2 = b.1;
            let d1 = (ehs - c1.0).powi(2) + (ehs_sq - c1.1).powi(2);
            let d2 = (ehs - c2.0).powi(2) + (ehs_sq - c2.1).powi(2);
            d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(idx, _)| idx as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::Evaluator;
    use std::env;
    use std::fs;

    struct MockEvaluator;
    impl Evaluator for MockEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 { 0u32 }
    }

    #[test]
    fn test_history_street_hash() {
        let builder = KMeansAbstraction::new(vec![(0.3,0.09),(0.7,0.49)], Arc::new(MockEvaluator));
        let h1 = builder.get_infoset_hash(&[0,1], &[], &[], 0);
        let h2 = builder.get_infoset_hash(&[0,1], &[], &[0], 0);
        assert_ne!(h1, h2);
    }
}
