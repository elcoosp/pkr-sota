# W1-T3: Compact CFR Table

## Objective
Implement the `CompactRegretTable` in `pkr-cfr` using `u8` quantized regrets (follow-the-leader strategy).

## Dependencies
- `pkr-contracts` (W0-T1)

## Exclusive File Paths
- `crates/pkr-cfr/Cargo.toml`
- `crates/pkr-cfr/src/table.rs`
- `crates/pkr-cfr/src/lib.rs`

## TDD Instructions
1. **Red**: In `table.rs`, write tests for adding regrets and retrieving the strategy. Test that adding positive regret to action A increases its probability. Test that quantization (u8) correctly clamps values at 0 and 255.
2. **Green**: Implement `CompactRegretTable` with `new(capacity, num_actions)`, `add_regret(infoset_idx, action_idx, delta)`, and `get_strategy(infoset_idx) -> Vec<f32>`. Use `Vec<u8>` for storage. Midpoint (128) is zero regret.
3. **Refactor**: Ensure memory is contiguous. Ensure `get_strategy` normalizes probabilities correctly so they sum to 1.0.

## Acceptance Criteria
- `cargo test -p pkr-cfr` passes.
- Regret table uses exactly 1 byte per action per infoset.
