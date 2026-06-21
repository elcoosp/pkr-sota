# W1-T2: Hand Evaluator

## Objective
Implement a fast 7-card hand evaluator in `pkr-eval` using a precomputed lookup table approach (e.g., Cactus Kev or similar).

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-core` (W1-T1)

## Exclusive File Paths
- `crates/pkr-eval/Cargo.toml`
- `crates/pkr-eval/src/lib.rs`
- `crates/pkr-eval/src/tables.rs`

## TDD Instructions
1. **Red**: In `lib.rs`, write tests for `NlheEvaluator::evaluate_hand()`. Test Royal Flush (returns highest score), Four of a Kind, Full House, and High Card. Assert relative ordering (higher rank = higher score).
2. **Green**: Implement the `NlheEvaluator` struct. Create a static or lazily-initialized lookup table in `tables.rs`. Map 7 cards to a `u16` rank.
3. **Refactor**: Optimize the lookup generation. Ensure `evaluate_hand` is `O(1)` after initialization.

## Acceptance Criteria
- `cargo test -p pkr-eval` passes.
- `NlheEvaluator` implements `pkr_contracts::Evaluator`.
