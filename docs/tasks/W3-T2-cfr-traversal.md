# W3-T2: CFR Traversal

## Objective
Implement the External Sampling MCCFR tree traversal in `pkr-cfr`.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-core` (W1-T1)
- `pkr-cfr` (W1-T3, W2-T1)

## Exclusive File Paths
- `crates/pkr-cfr/src/traversal.rs`
- `crates/pkr-cfr/src/lib.rs`

## TDD Instructions
1. **Red**: In `traversal.rs`, write tests for `run_iteration`. Mock a simple 2-step game tree. Assert that calling `run_iteration` updates the `CompactRegretTable` and that regrets move towards the Nash equilibrium for the mock game.
2. **Green**: Implement `run_iteration(rules: &dyn GameRules, table: &mut CompactRegretTable, abstraction: &dyn AbstractionBuilder, rng: &mut impl Rng)`. Traverse the tree recursively. At chance nodes, sample one outcome. At player nodes, sample one action based on current strategy. Update regrets using DCFR logic.
3. **Refactor**: Ensure recursion depth is safe. Use `rand` for sampling.

## Acceptance Criteria
- `cargo test -p pkr-cfr` passes.
- Correctly implements External Sampling MCCFR.
