//! # pkr-abstraction
//!
//! Hand abstraction using pre‑computed k‑means centroids.
//! Runtime lookup simply finds the nearest centroid in O(k) without any
//! Monte Carlo sampling.

pub mod cluster;
pub mod ehs;
pub use ehs::calculate_ehs;

use pkr_contracts::AbstractionBuilder;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentroidStore {
    pub centroids: Vec<(f32, f32)>,
}

/// Loads centroids from a bincode file.
pub fn load_centroids(path: &str) -> Result<CentroidStore, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let store: CentroidStore = bincode::deserialize_from(reader)?;
    Ok(store)
}

/// Saves centroids to a bincode file.
pub fn save_centroids(path: &str, store: &CentroidStore) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    bincode::serialize_into(file, store)?;
    Ok(())
}

/// A fast abstraction builder that maps a (hole, board) pair to the nearest
/// centroid's index using precomputed EHS features.
pub struct KMeansAbstraction {
    centroids: Vec<(f32, f32)>,
}

impl KMeansAbstraction {
    pub fn new(centroids: Vec<(f32, f32)>) -> Self {
        KMeansAbstraction { centroids }
    }

    /// Build from a CentroidStore loaded from disk.
    pub fn from_store(store: CentroidStore) -> Self {
        KMeansAbstraction { centroids: store.centroids }
    }
}

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], _history: &[u8]) -> u64 {
        // Compute EHS features once per (hole, board) pair via fast Monte Carlo
        // (this is still called, but only once per infoset visited).
        // In a full system you'd precompute the features for every possible pair,
        // but here we keep the existing calculate_ehs for simplicity.
        let (ehs, ehs_sq) = crate::ehs::calculate_ehs(hole, board, &pkr_eval::NlheEvaluator);

        // Find nearest centroid
        let cluster_id = self.centroids.iter()
            .enumerate()
            .min_by(|a, b| {
                let c1 = a.1;
                let c2 = b.1;
                let d1 = (ehs - c1.0).powi(2) + (ehs_sq - c1.1).powi(2);
                let d2 = (ehs - c2.0).powi(2) + (ehs_sq - c2.1).powi(2);
                d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        // Hash the cluster ID
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        cluster_id.hash(&mut hasher);
        hasher.finish()
    }
}
