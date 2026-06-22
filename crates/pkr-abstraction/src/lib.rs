pub mod ehs;
pub use ehs::calculate_ehs;

use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_eval::lookup::{choose, combinadic_rank};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::hash::{BuildHasher, BuildHasherDefault, Hasher};
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
    tables: HashMap<u8, OnceLock<Mmap>>, // street codes 0..3
    evaluator: Arc<dyn Evaluator>,
}

impl KMeansAbstraction {
    pub fn new(
        default_centroids: Vec<(f32, f32)>,
        evaluator: Arc<dyn Evaluator>,
    ) -> Self {
        let mut tables = HashMap::new();
        for s in 0u8..=3 {
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

    pub fn init_table(&self, street_code: u8, path: &str) -> Result<(), std::io::Error> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let lock = self.tables.get(&street_code).expect("table slot not created");
        lock.set(mmap).map_err(|_| std::io::Error::new(std::io::ErrorKind::AlreadyExists, "table already set"))?;
        Ok(())
    }

    /// Preflop: hole cards only, index = combinadic_rank_2(hole)
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
        let mut found = false;
        for (mi, pos) in masks.iter().enumerate() {
            let h1 = all[pos[0]]; let h2 = all[pos[1]];
            if hole_set.contains(&h1) && hole_set.contains(&h2) {
                mask_idx = mi;
                found = true;
                break;
            }
        }
        assert!(found, "hole not found in 5-card set");
        combo_idx * 10 + mask_idx
    }

    fn flat_index_turn(hole: &[u8], board: &[u8]) -> usize {
        assert_eq!(hole.len(), 2);
        assert_eq!(board.len(), 4);
        let mut all = [0u8; 6];
        all[0] = hole[0]; all[1] = hole[1];
        all[2] = board[0]; all[3] = board[1]; all[4] = board[2]; all[5] = board[3];
        all.sort_unstable_by(|a, b| b.cmp(a));
        let rank = combinadic_rank_6(&all);
        let hole_set = [hole[0], hole[1]];
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

fn combinadic_rank_6(cards: &[u8; 6]) -> u64 {
    let c0 = cards[0] as u32; let c1 = cards[1] as u32;
    let c2 = cards[2] as u32; let c3 = cards[3] as u32;
    let c4 = cards[4] as u32; let c5 = cards[5] as u32;
    choose(c0, 6) as u64 + choose(c1, 5) as u64 + choose(c2, 4) as u64
        + choose(c3, 3) as u64 + choose(c4, 2) as u64 + choose(c5, 1) as u64
}

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64 {
        let centroids = self.centroids.get(&street)
            .unwrap_or(&self.default_centroids);

        let cluster_id = match board.len() {
            0 => {
                if let Some(table) = self.tables.get(&0u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_preflop(hole) as usize;
                    table[idx] as u64
                } else {
                    let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
                    nearest_centroid(ehs, ehs_sq, centroids)
                }
            },
            3 => {
                if let Some(table) = self.tables.get(&1u8).and_then(|l| l.get()) {
                    let idx = Self::flat_index_flop(hole, board);
                    table[idx] as u64
                } else {
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
            _ => {
                let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
                nearest_centroid(ehs, ehs_sq, centroids)
            }
        };

        // Strong hash combining street, history, and cluster_id
        let hasher = BuildHasherDefault::<std::collections::hash_map::DefaultHasher>::default();
        let mut h = hasher.build_hasher();
        h.write_u8(street);
        h.write(history);
        h.write_u64(cluster_id);
        h.finish()
    }
}

fn nearest_centroid(ehs: f32, ehs_sq: f32, centroids: &[(f32, f32)]) -> u64 {
    centroids.iter()
        .enumerate()
        .min_by(|a, b| {
            let c1 = a.1; let c2 = b.1;
            let dx1 = ehs - c1.0; let dy1 = ehs_sq - c1.1;
            let dx2 = ehs - c2.0; let dy2 = ehs_sq - c2.1;
            (dx1*dx1 + dy1*dy1).total_cmp(&(dx2*dx2 + dy2*dy2))
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
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 { 0u32 }
    }

    #[test]
    fn test_history_street_hash() {
        let builder = KMeansAbstraction::new(vec![(0.3,0.09),(0.7,0.49)], Arc::new(MockEvaluator));
        let h1 = builder.get_infoset_hash(&[0,1], &[], &[], 0);
        let h2 = builder.get_infoset_hash(&[0,1], &[], &[0], 0);
        assert_ne!(h1, h2, "history must change hash");
    }

    #[test]
    fn test_flat_index_flop() {
        let idx = KMeansAbstraction::flat_index_flop(&[0,1], &[2,3,4]);
        assert!(idx < 25_989_600);
        let idx2 = KMeansAbstraction::flat_index_flop(&[1,0], &[2,3,4]);
        assert_eq!(idx, idx2);
    }

    #[test]
    fn test_preflop_index() {
        // choose(1,2)+choose(0,1) for hole [1,0] sorted to [1,0]
        let idx = KMeansAbstraction::flat_index_preflop(&[1,0]);
        assert_eq!(idx, 0); // first combo
        let idx2 = KMeansAbstraction::flat_index_preflop(&[0,1]);
        assert_eq!(idx, idx2);
    }
}
