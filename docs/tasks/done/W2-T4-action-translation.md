# W2-T4: Action Translation

## Objective
Implement the pseudo-harmonic mapping for off-tree action translation in `pkr-export`.

## Dependencies
- `pkr-contracts` (W0-T1)

## Exclusive File Paths
- `crates/pkr-export/src/translate.rs`
- `crates/pkr-export/src/lib.rs`

## TDD Instructions
1. **Red**: In `translate.rs`, write tests for `compute_translation`. Given `lower_action=0.33`, `upper_action=0.50`, `actual_action=0.42`, and `reach_prob_lower=0.6`, `reach_prob_upper=0.4`, assert that the returned probabilities sum to 1.0 and favor the lower action correctly according to the pseudo-harmonic formula.
2. **Green**: Implement `compute_translation(lower: f32, upper: f32, actual: f32, reach_lower: f32, reach_upper: f32) -> (u8, u8)`. Formula: `P(lower) = 1 - ((actual - lower) * reach_upper) / ((upper - actual) * reach_lower + (actual - lower) * reach_upper)`.
3. **Refactor**: Ensure division by zero is handled safely. Scale results to `u8` (0-255).

## Acceptance Criteria
- `cargo test -p pkr-export` passes.
- Correctly implements Ganzfried & Sandholm (2013) pseudo-harmonic mapping.
