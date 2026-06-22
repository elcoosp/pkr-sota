pub mod cluster;
pub mod ehs;
pub use ehs::calculate_ehs;

use pkr_contracts::AbstractionBuilder;
use pkr_contracts::Evaluator;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;

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
    centroids: Vec<(f32, f32)>,
    evaluator: Arc<dyn Evaluator>,
    cache: RwLock<HashMap<(u64, u64), u64>>,
}

impl KMeansAbstraction {
    pub fn new(centroids: Vec<(f32, f32)>, evaluator: Arc<dyn Evaluator>) -> Self {
        KMeansAbstraction {
            centroids,
            evaluator,
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn from_store(store: CentroidStore, evaluator: Arc<dyn Evaluator>) -> Self {
        Self::new(store.centroids, evaluator)
    }

    fn hash_cards(cards: &[u8]) -> u64 {
        let mut h: u64 = 14695981039346656037;
        for &c in cards {
            h ^= c as u64;
            h = h.wrapping_mul(1099511628211);
        }
        h
    }

    /// Combine street, history bytes and cluster_id into a single u64.
    fn combine_hash(street: u8, history: &[u8], cluster_id: u64) -> u64 {
        let mut h: u64 = 0x9E3779B97F4A7C15; // start non-zero
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

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64 {
        let hole_hash = Self::hash_cards(hole);
        let board_hash = Self::hash_cards(board);

        let cluster_id = {
            if let Ok(cache) = self.cache.read() {
                if let Some(&id) = cache.get(&(hole_hash, board_hash)) {
                    id
                } else {
                    drop(cache);
                    let (ehs, ehs_sq) = calculate_ehs(hole, board, self.evaluator.as_ref());
                    let id = self.centroids.iter()
                        .enumerate()
                        .min_by(|a, b| {
                            let c1 = a.1;
                            let c2 = b.1;
                            let d1 = (ehs - c1.0).powi(2) + (ehs_sq - c1.1).powi(2);
                            let d2 = (ehs - c2.0).powi(2) + (ehs_sq - c2.1).powi(2);
                            d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|(idx, _)| idx as u64)
                        .unwrap_or(0);
                    if let Ok(mut cache) = self.cache.write() {
                        cache.insert((hole_hash, board_hash), id);
                    }
                    id
                }
            } else {
                0
            }
        };

        Self::combine_hash(street, history, cluster_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::Evaluator;
    use std::sync::Arc;

    struct MockEvaluator;
    impl Evaluator for MockEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 {
            0u32
        }
    }

    #[test]
    fn test_abstraction_includes_history_and_street() {
        let centroids = vec![(0.3, 0.09), (0.7, 0.49)];
        let builder = KMeansAbstraction::new(centroids, Arc::new(MockEvaluator));

        let h1 = builder.get_infoset_hash(&[0, 1], &[], &[], 0);
        let h2 = builder.get_infoset_hash(&[0, 1], &[], &[0], 0);
        assert_ne!(h1, h2, "history must change hash");

        let h3 = builder.get_infoset_hash(&[0, 1], &[], &[], 1);
        assert_ne!(h1, h3, "street must change hash");

        // Both h1,h2 should be non-zero
        assert!(h1 != 0 && h2 != 0, "hashes should not be zero");
    }

    #[test]
    fn test_abstraction_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<KMeansAbstraction>();
    }
}
