# W3-T1: Abstraction Clustering

## Objective
Implement K-Means clustering to bucket hands into infosets in `pkr-abstraction`.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-core` (W1-T1)
- `pkr-eval` (W1-T2)
- `pkr-abstraction` (W2-T2)

## Exclusive File Paths
- `crates/pkr-abstraction/src/cluster.rs`
- `crates/pkr-abstraction/src/lib.rs`

## TDD Instructions
1. **Red**: In `cluster.rs`, write tests for `cluster_hands`. Generate features for a small subset of hands (e.g., 100 hands). Assert that `cluster_hands` returns a valid `InfoSet` hash for each hand and that the number of unique hashes does not exceed `k`.
2. **Green**: Implement `cluster_hands(features: Vec<(f32, f32)>, k: usize) -> Vec<u64>`. Use a basic K-Means algorithm. Assign each hand to a cluster centroid. The cluster ID becomes part of the `InfoSet` hash.
3. **Refactor**: Use `rayon` to parallelize distance calculations. Ensure deterministic initialization of centroids for reproducibility.

## Acceptance Criteria
- `cargo test -p pkr-abstraction` passes.
- Implements `pkr_contracts::AbstractionBuilder`.
