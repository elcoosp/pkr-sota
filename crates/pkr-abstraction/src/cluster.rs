//! K-Means clustering for hand abstraction.

use pkr_contracts::{AbstractionBuilder, Evaluator};
use rayon::prelude::*;
use std::hash::{Hash, Hasher};

/// Compute Euclidean distance between two 2D points.
fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

/// A small helper to hash a usize into a u64.
fn hash_cluster_id(id: usize) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

/// Run K-Means on a batch of (EHS, EHS²) features and return cluster hashes.
/// The cluster ID is hashed to produce the InfoSet hash.
/// Centroids are initialized deterministically by taking `k` evenly spaced
/// points from the sorted feature list.
pub fn cluster_hands(features: Vec<(f32, f32)>, k: usize) -> Vec<u64> {
    let n = features.len();
    if n == 0 {
        return Vec::new();
    }
    let k = k.min(n); // never more clusters than points

    // Sort features by EHS for deterministic initialization
    let mut sorted = features.clone();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Initialize centroids: pick k evenly spaced indices
    let mut centroids: Vec<(f32, f32)> = (0..k)
        .map(|i| {
            let idx = (i * (sorted.len() - 1)) / (k - 1).max(1);
            sorted[idx]
        })
        .collect();

    let mut assignments = vec![0usize; n];
    let max_iters = 100;

    for _ in 0..max_iters {
        // Assignment step (parallel)
        let new_assignments: Vec<usize> = features
            .par_iter()
            .map(|&f| {
                centroids
                    .iter()
                    .enumerate()
                    .min_by(|&(_, &c1), &(_, &c2)| {
                        distance(f, c1)
                            .partial_cmp(&distance(f, c2))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(idx, _)| idx)
                    .unwrap_or(0)
            })
            .collect();

        // Update step
        let mut sums = vec![(0.0f32, 0.0f32); k];
        let mut counts = vec![0usize; k];
        for (&f, &c) in features.iter().zip(new_assignments.iter()) {
            sums[c].0 += f.0;
            sums[c].1 += f.1;
            counts[c] += 1;
        }

        let mut changed = false;
        for i in 0..k {
            if counts[i] > 0 {
                let new_centroid = (sums[i].0 / counts[i] as f32, sums[i].1 / counts[i] as f32);
                if distance(new_centroid, centroids[i]) > 1e-6 {
                    changed = true;
                }
                centroids[i] = new_centroid;
            }
            // else keep unchanged (rare empty cluster)
        }

        assignments = new_assignments;

        if !changed {
            break;
        }
    }

    // Build final hashes
    assignments.into_iter().map(hash_cluster_id).collect()
}

/// A pre‑trained K‑Means abstraction builder.
pub struct KMeansAbstraction {
    centroids: Vec<(f32, f32)>,
    /// Evaluator used to compute EHS
    evaluator: Box<dyn Evaluator>,
}

impl KMeansAbstraction {
    /// Create a new abstraction builder from trained centroids.
    pub fn new(centroids: Vec<(f32, f32)>, evaluator: Box<dyn Evaluator>) -> Self {
        KMeansAbstraction {
            centroids,
            evaluator,
        }
    }
}

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], _history: &[u8]) -> u64 {
        // Compute EHS using the existing function from this crate
        let (ehs, ehs_sq) = crate::ehs::calculate_ehs(hole, board, self.evaluator.as_ref());
        // Find nearest centroid
        let feat = (ehs, ehs_sq);
        let cluster_id = self
            .centroids
            .iter()
            .enumerate()
            .min_by(|&(_, &c1), &(_, &c2)| {
                distance(feat, c1)
                    .partial_cmp(&distance(feat, c2))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0);
        hash_cluster_id(cluster_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::AbstractionBuilder;
    use pkr_eval::NlheEvaluator;

    /// Helper to generate dummy features (100 hands) for testing.
    fn generate_features(n: usize) -> Vec<(f32, f32)> {
        (0..n)
            .map(|i| {
                let eh = (i as f32 / n as f32) * 0.8 + 0.1;
                (eh, eh * eh)
            })
            .collect()
    }

    #[test]
    fn test_cluster_hands_smoke() {
        let features = generate_features(100);
        let k = 5;
        let hashes = cluster_hands(features, k);
        assert_eq!(hashes.len(), 100);
        let unique: std::collections::HashSet<u64> = hashes.into_iter().collect();
        assert!(
            unique.len() <= k,
            "Too many unique clusters: {}",
            unique.len()
        );
    }

    #[test]
    fn test_cluster_hands_deterministic() {
        let features = generate_features(50);
        let hashes1 = cluster_hands(features.clone(), 3);
        let hashes2 = cluster_hands(features, 3);
        assert_eq!(hashes1, hashes2, "Clustering must be deterministic");
    }

    #[test]
    fn test_cluster_hands_k_greater_than_points() {
        let features = generate_features(10);
        let hashes = cluster_hands(features, 20);
        assert_eq!(hashes.len(), 10);
        let unique: std::collections::HashSet<u64> = hashes.into_iter().collect();
        assert!(unique.len() <= 10);
    }

    #[test]
    fn test_empty_features() {
        let features: Vec<(f32, f32)> = vec![];
        let hashes = cluster_hands(features, 5);
        assert!(hashes.is_empty());
    }

    #[test]
    fn test_abstraction_builder_basic() {
        let centroids = vec![(0.2, 0.04), (0.5, 0.25), (0.8, 0.64), (0.95, 0.9025)];
        let evaluator = Box::new(NlheEvaluator);
        let builder = KMeansAbstraction::new(centroids, evaluator);

        // AA preflop
        let hole = vec![
            (0 * 13 + 12), // A♠
            (1 * 13 + 12), // A♥
        ];
        let board = vec![];
        let hash = builder.get_infoset_hash(&hole, &board, &[]);
        assert!(hash != 0, "Hash should be non-zero for AA");
        let hash2 = builder.get_infoset_hash(&hole, &board, &[]);
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_k_means_convergence() {
        // Create two well-separated groups
        let mut features = Vec::new();
        for _ in 0..50 {
            features.push((0.2 + rand::random::<f32>() * 0.01, 0.04));
            features.push((0.8 + rand::random::<f32>() * 0.01, 0.64));
        }
        let hashes = cluster_hands(features, 2);
        assert_eq!(hashes.len(), 100);
        let unique: std::collections::HashSet<u64> = hashes.into_iter().collect();
        assert_eq!(unique.len(), 2, "Should have exactly 2 clusters");
    }
}
