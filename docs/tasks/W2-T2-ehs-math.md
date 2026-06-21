# W2-T2: EHS Math

## Objective
Implement Expected Hand Strength (EHS) and EHS² (variance proxy) in `pkr-abstraction`.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-core` (W1-T1)
- `pkr-eval` (W1-T2)

## Exclusive File Paths
- `crates/pkr-abstraction/src/ehs.rs`
- `crates/pkr-abstraction/src/lib.rs`

## TDD Instructions
1. **Red**: In `ehs.rs`, write tests for `calculate_ehs`. Mock a scenario where a player has pocket Aces on a random board. Assert that EHS is high (close to 0.85+). Test that EHS² is calculated correctly as the variance of equity across board runouts.
2. **Green**: Implement `calculate_ehs(hole: &[u8], board: &[u8], evaluator: &dyn Evaluator) -> (f32, f32)`. Use Monte Carlo simulation over remaining deck cards to compute equity and equity squared.
3. **Refactor**: Use `rand` crate for sampling. Parallelize the simulation using `rayon` if performance requires it, but keep it simple first.

## Acceptance Criteria
- `cargo test -p pkr-abstraction` passes.
- Returns `(ehs, ehs_squared)` as `f32` values in `[0.0, 1.0]`.
