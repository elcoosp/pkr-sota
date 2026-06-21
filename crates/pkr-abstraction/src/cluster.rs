//! K-Means clustering for hand abstraction.
//! Will be implemented in Green phase.

use pkr_contracts::{AbstractionBuilder, InfoSet};

/// Placeholder: cluster a batch of (EHS, EHS²) features into `k` clusters.
/// Returns a hash for each hand (cluster ID).
pub fn cluster_hands(_features: Vec<(f32, f32)>, _k: usize) -> Vec<u64> {
    unimplemented!("cluster_hands not yet implemented (Red phase)")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Will panic because unimplemented!
        assert_eq!(hashes.len(), 100);
        let unique: std::collections::HashSet<u64> = hashes.into_iter().collect();
        assert!(
            unique.len() <= k,
            "Too many unique clusters: {}",
            unique.len()
        );
    }
}
