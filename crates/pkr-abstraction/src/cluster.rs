//! K-Means clustering for hand abstraction.

use pkr_contracts::{AbstractionBuilder, Evaluator};
use rayon::prelude::*;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::RwLock;

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
pub fn cluster_hands(features: Vec<(f32, f32)>, k: usize) -> Vec<u64> {
    let n = features.len();
    if n == 0 {
        return Vec::new();
    }
    let k = k.min(n);

    let mut sorted = features.clone();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut centroids: Vec<(f32, f32)> = (0..k)
        .map(|i| {
            let idx = (i * (sorted.len() - 1)) / (k - 1).max(1);
            sorted[idx]
        })
        .collect();

    let mut assignments = vec![0usize; n];
    let max_iters = 100;

    for _ in 0..max_iters {
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
        }

        assignments = new_assignments;

        if !changed {
            break;
        }
    }

    assignments.into_iter().map(hash_cluster_id).collect()
}

/// A pre‑trained K‑Means abstraction builder.
pub struct KMeansAbstraction {
    centroids: Vec<(f32, f32)>,
    evaluator: Box<dyn Evaluator>,
    cache: RwLock<HashMap<(u64, u64), u64>>,
}

impl KMeansAbstraction {
    pub fn new(centroids: Vec<(f32, f32)>, evaluator: Box<dyn Evaluator>) -> Self {
        KMeansAbstraction {
            centroids,
            evaluator,
            cache: RwLock::new(HashMap::new()),
        }
    }
}

impl AbstractionBuilder for KMeansAbstraction {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], _history: &[u8]) -> u64 {
        let mut h1: u64 = 14695981039346656037;
        for &c in hole {
            h1 ^= c as u64;
            h1 = h1.wrapping_mul(1099511628211);
        }
        let mut h2: u64 = 14695981039346656037;
        for &c in board {
            h2 ^= c as u64;
            h2 = h2.wrapping_mul(1099511628211);
        }

        if let Ok(cache) = self.cache.read() {
            if let Some(&val) = cache.get(&(h1, h2)) {
                return val;
            }
        }

        let (ehs, ehs_sq) = crate::ehs::calculate_ehs(hole, board, self.evaluator.as_ref());
        let feat = (ehs, ehs_sq);

        let cluster_id = self
            .centroids
            .iter()
            .enumerate()
            .min_by(|&(_, &c1), &(_, &c2)| {
                let d1 = (feat.0 - c1.0).powi(2) + (feat.1 - c1.1).powi(2);
                let d2 = (feat.0 - c2.0).powi(2) + (feat.1 - c2.1).powi(2);
                d1.partial_cmp(&d2).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0);

        let hash = hash_cluster_id(cluster_id);
        if let Ok(mut cache) = self.cache.write() {
            cache.insert((h1, h2), hash);
        }
        hash
    }
}
