pub mod cluster;
pub mod ehs;
pub use ehs::calculate_ehs;

use pkr_contracts::AbstractionBuilder;
use pkr_eval::lookup::combinadic_rank;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
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

/// Fast abstraction builder that uses a precomputed mmap'd lookup table.
/// The table maps (hole + board) as a 5-card combination to a cluster_id.
pub struct KMeansAbstraction {
    table_mmap: Option<Mmap>,
}

impl KMeansAbstraction {
    pub fn new() -> Self {
        KMeansAbstraction { table_mmap: None }
    }

    pub fn from_store(_store: CentroidStore) -> Self {
        Self::new()
    }

    pub fn load_table(&mut self, path: &str) -> Result<(), std::io::Error> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        self.table_mmap = Some(mmap);
        Ok(())
    }

    /// Flat index for a given hole (2 cards) + board (3 cards).
    fn flat_index(hole: &[u8], board: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert!(board.len() == 3, "only flop supported for table lookup");
        let mut all = [0u8; 5];
        all[0] = hole[0];
        all[1] = hole[1];
        all[2] = board[0];
        all[3] = board[1];
        all[4] = board[2];
        all.sort_unstable_by(|a, b| b.cmp(a)); // descending

        let combo_idx = combinadic_rank(&all) as usize;

        // Determine mask index (which 2 of the 5 are the hole cards)
        let hole_set = [hole[0], hole[1]];
        let mut mask_idx = 0;
        for (mi, pos) in [
            [0usize,1], [0,2], [0,3], [0,4],
            [1,2], [1,3], [1,4], [2,3], [2,4], [3,4],
        ].iter().enumerate() {
            let h1 = all[pos[0]];
            let h2 = all[pos[1]];
            if hole_set.contains(&h1) && hole_set.contains(&h2) {
                mask_idx = mi;
                break;
            }
        }

        combo_idx * 10 + mask_idx
    }
}

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64 {
        let cluster_id = if let Some(mmap) = &self.table_mmap {
            let idx = Self::flat_index(hole, board);
            mmap[idx] as u64
        } else {
            // Fallback: return a constant (no abstraction) if table not loaded.
            0
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

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::Evaluator;

    struct MockEvaluator;
    impl Evaluator for MockEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 { 0u32 }
    }

    #[test]
    fn test_abstraction_includes_history_and_street_no_table() {
        let builder = KMeansAbstraction::new();
        let h1 = builder.get_infoset_hash(&[0, 1], &[], &[], 0);
        let h2 = builder.get_infoset_hash(&[0, 1], &[], &[0], 0);
        assert_ne!(h1, h2);
        let h3 = builder.get_infoset_hash(&[0, 1], &[], &[], 1);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_flat_index_in_bounds() {
        let hole = [0u8, 1];
        let board = [2, 3, 4];
        let idx = KMeansAbstraction::flat_index(&hole, &board);
        assert!(idx < 25_989_600);
    }
}
