//! K-Means clustering utilities.
use rayon::prelude::*;

/// Euclidean distance between two 2D points.
fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

/// Run K-Means on a batch of (EHS, EHS²) features and return cluster indices.
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
                    .min_by(|a, b| {
                        let c1 = a.1;
                        let c2 = b.1;
                        distance(f, *c1)
                            .partial_cmp(&distance(f, *c2))
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

    assignments.into_iter().map(|idx| idx as u64).collect()
}
