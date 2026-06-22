pub mod cluster;
pub mod ehs;
pub use ehs::calculate_ehs;

use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_eval::lookup::combinadic_rank;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
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

/// Fast abstraction with per‑street centroids and optional precomputed flop/turn/river tables.
pub struct KMeansAbstraction {
    centroids: HashMap<u8, Vec<(f32, f32)>>,
    default_centroids: Vec<(f32, f32)>,
    tables: HashMap<u8, Mmap>,
    evaluator: Arc<dyn Evaluator>,
}

impl KMeansAbstraction {
    pub fn new(
        default_centroids: Vec<(f32, f32)>,
        evaluator: Arc<dyn Evaluator>,
    ) -> Self {
        KMeansAbstraction {
            centroids: HashMap::new(),
            default_centroids,
            tables: HashMap::new(),
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

    pub fn load_street_table(&mut self, street_code: u8, path: &str) -> Result<(), std::io::Error> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        self.tables.insert(street_code, mmap);
        Ok(())
    }

    fn flat_index(hole: &[u8], board: &[u8]) -> usize {
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
}

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64 {
        let centroids = self.centroids.get(&street)
            .unwrap_or(&self.default_centroids);

        let cluster_id = if board.len() == 3 && self.tables.contains_key(&1u8) {
            let idx = Self::flat_index(hole, board);
            self.tables.get(&1).unwrap()[idx] as u64
        } else {
            let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
            nearest_centroid(ehs, ehs_sq, centroids)
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

#[inline]
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

    #[test]
    fn test_per_street_centroids() {
        let mut builder = KMeansAbstraction::new(vec![(0.5,0.25)], Arc::new(MockEvaluator));
        let dir = env::temp_dir();
        let path = dir.join("test_centroids.bin");
        save_centroids(path.to_str().unwrap(), &CentroidStore { centroids: vec![(0.1,0.01)] }).unwrap();
        builder.load_street_centroids(1, path.to_str().unwrap()).unwrap();
        let _ = fs::remove_file(&path); // cleanup
        let h = builder.get_infoset_hash(&[0,1], &[2,3,4], &[], 1);
        assert!(h != 0);
    }
}
