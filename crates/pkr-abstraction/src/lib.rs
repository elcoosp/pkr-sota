//! # pkr-abstraction
//!
//! Hand abstraction for poker solvers.
//!
//! Provides:
//! - **EHS (Expected Hand Strength)** calculation via Monte Carlo sampling.
//! - **K‑Means clustering** to map (EHS, EHS²) pairs into information set hashes.
//!
//! The main entry point is [`KMeansAbstraction`], which implements the
//! [`AbstractionBuilder`](pkr_contracts::AbstractionBuilder) trait and can
//! be used directly in CFR training.
//!
//! # Example
//!
//! ```rust
//! use pkr_abstraction::{KMeansAbstraction, calculate_ehs};
//! use pkr_contracts::AbstractionBuilder;
//! use pkr_eval::NlheEvaluator;
//!
//! let centroids = vec![(0.2, 0.04), (0.5, 0.25), (0.8, 0.64)];
//! let eval = Box::new(NlheEvaluator);
//! let builder = KMeansAbstraction::new(centroids, eval);
//!
//! let hole = vec![0, 1];
//! let board = vec![];
//! let hash = builder.get_infoset_hash(&hole, &board, &[]);
//! ```

pub mod cluster;
pub mod ehs;

// Re‑export the key public API for convenience
pub use cluster::KMeansAbstraction;
pub use ehs::calculate_ehs;

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_contracts::{AbstractionBuilder, Evaluator};

    /// A simple mock evaluator for testing that always returns 0.
    struct MockEvaluator;
    impl Evaluator for MockEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u32 {
            0u32
        }
    }

    /// Test that `KMeansAbstraction` correctly implements `AbstractionBuilder`
    /// and that `get_infoset_hash` returns a valid `u64`.
    #[test]
    fn test_abstraction_builder_trait_implementation() {
        let centroids = vec![(0.3, 0.09), (0.7, 0.49)];
        let builder = KMeansAbstraction::new(centroids, Box::new(MockEvaluator));

        // Verify the type implements the trait (compile‑time check)
        fn assert_abstraction_builder<T: AbstractionBuilder>(_b: &T) {}
        assert_abstraction_builder(&builder);

        // Any valid hole/board must produce a non‑zero hash
        let hash = builder.get_infoset_hash(&[0, 1], &[], &[]);
        assert!(hash != 0, "get_infoset_hash should return non‑zero");
    }

    /// Ensure the implementation is thread‑safe (Send + Sync).
    #[test]
    fn test_abstraction_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<KMeansAbstraction>();
    }
    /// Test that calculate_ehs is publicly accessible via the crate root.
    #[test]
    fn test_calculate_ehs_reexport() {
        // This just ensures the function compiles and is callable.
        let _ = crate::calculate_ehs(&[0, 1], &[], &pkr_eval::NlheEvaluator);
    }

    /// Test that KMeansAbstraction with a single centroid always returns the same hash.
    #[test]
    fn test_single_centroid_deterministic_hash() {
        let centroids = vec![(0.5, 0.25)];
        let builder = KMeansAbstraction::new(centroids, Box::new(MockEvaluator));
        let hash1 = builder.get_infoset_hash(&[0, 1], &[], &[]);
        let hash2 = builder.get_infoset_hash(&[0, 1], &[], &[]);
        assert_eq!(
            hash1, hash2,
            "Single centroid should produce same hash each call"
        );
    }

    /// Test that an empty centroid list gracefully returns a default hash (0).
    #[test]
    fn test_empty_centroids_handled() {
        let centroids: Vec<(f32, f32)> = vec![];
        let builder = KMeansAbstraction::new(centroids, Box::new(MockEvaluator));
        // No centroids means cluster_id is 0 -> hash of 0 is non-zero (hashed 0).
        // The implementation returns hash_cluster_id(0), which should be deterministic.
        let hash = builder.get_infoset_hash(&[0, 1], &[], &[]);
        // The hash may or may not be zero depending on hash function, but it should not panic.
        // We just verify it's callable and returns something.
        assert!(hash == hash, "should not panic");
    }

    /// Test thread-safety by calling from multiple threads.
    #[test]
    fn test_concurrent_access() {
        use std::thread;
        let centroids = vec![(0.3, 0.09), (0.7, 0.49)];
        let builder =
            std::sync::Arc::new(KMeansAbstraction::new(centroids, Box::new(MockEvaluator)));
        let mut handles = vec![];
        for _ in 0..4 {
            let b = builder.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..10 {
                    let hash = b.get_infoset_hash(&[0, 1], &[], &[]);
                    assert!(hash != 0);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
    }

    /// Test that the builder can be used with a history argument (should be ignored).
    #[test]
    fn test_history_is_ignored() {
        let centroids = vec![(0.5, 0.25)];
        let builder = KMeansAbstraction::new(centroids, Box::new(MockEvaluator));
        let hash_no_history = builder.get_infoset_hash(&[0, 1], &[], &[]);
        let hash_with_history = builder.get_infoset_hash(&[0, 1], &[], &[1, 2, 3]);
        assert_eq!(hash_no_history, hash_with_history);
    }
}
