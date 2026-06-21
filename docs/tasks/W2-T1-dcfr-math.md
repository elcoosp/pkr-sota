# W2-T1: DCFR Math

## Objective
Implement the Discounted CFR (DCFR) update logic in `pkr-cfr`.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-cfr` (W1-T3)

## Exclusive File Paths
- `crates/pkr-cfr/src/dcfr.rs`
- `crates/pkr-cfr/src/lib.rs`

## TDD Instructions
1. **Red**: In `dcfr.rs`, write tests for the `update_regret` function. Verify that positive regrets are discounted by `t^1.5 / (t^1.5 + 1)` and negative regrets by `t^0 / (t^0 + 1)`. Test edge cases like `t=0` and `t=1`.
2. **Green**: Implement `update_regret(current: u8, iteration: u32, delta: f32, is_positive: bool) -> u8`. Use the formula `t^a / (t^a + 1)` where `a=1.5` for positive and `a=0.0` for negative. Ensure the result is clamped to `[0, 255]`.
3. **Refactor**: Use `f32::powf` for the calculation. Ensure no floating point NaNs can occur.

## Acceptance Criteria
- `cargo test -p pkr-cfr` passes.
- DCFR formula matches Brown & Sandholm (2019) with alpha=1.5, beta=0.
