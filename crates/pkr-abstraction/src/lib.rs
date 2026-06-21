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
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u16 {
            0
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
}
