# W4-T1: Abstraction Crate API

## Objective
Finalize the `pkr-abstraction` crate by exposing the public API and ensuring the `AbstractionBuilder` trait is fully implemented and documented.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-core` (W1-T1)
- `pkr-eval` (W1-T2)
- `pkr-abstraction` (W2-T2, W3-T1)

## Exclusive File Paths
- `crates/pkr-abstraction/src/lib.rs`

## TDD Instructions
1. **Red**: In `lib.rs`, write a test that creates an instance of your K-Means abstraction builder. Assert that it correctly implements the `pkr_contracts::AbstractionBuilder` trait by calling `get_infoset_hash` on a mock hand and board, verifying it returns a valid `u64`.
2. **Green**: Re-export the `cluster` and `ehs` modules. Ensure the struct holding the K-Means centroids implements `AbstractionBuilder`. Add Rustdoc comments explaining how to initialize and use the abstraction builder.
3. **Refactor**: Ensure no internal state is mutated during `get_infoset_hash` (it should be thread-safe, taking `&self`).

## Acceptance Criteria
- `cargo test -p pkr-abstraction` passes.
- `cargo doc -p pkr-abstraction --no-deps` generates without warnings.
- Public API is clean and fully documented.
