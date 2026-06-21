# W4-T2: CFR Crate API

## Objective
Expose the public API for the CFR trainer in `pkr-cfr` by creating a high-level `Trainer` struct.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-core` (W1-T1)
- `pkr-cfr` (W1-T3, W2-T1, W3-T2)

## Exclusive File Paths
- `crates/pkr-cfr/src/lib.rs`

## TDD Instructions
1. **Red**: In `lib.rs`, write a test that initializes a `Trainer` with `NlheRuleset` and a mock `AbstractionBuilder`. Call `trainer.run_iteration()` and assert that the `CompactRegretTable` is updated without panicking.
2. **Green**: Create a `Trainer` struct. Implement `Trainer::new(rules: Box<dyn GameRules>, abstraction: Box<dyn AbstractionBuilder>) -> Self`. Implement `Trainer::run_iteration(&mut self, rng: &mut impl Rng)` which internally calls the `traversal` logic. Add a method `Trainer::get_table(&self) -> &CompactRegretTable`.
3. **Refactor**: Ensure the `Trainer` struct owns the `CompactRegretTable`. Add Rustdoc comments explaining the DCFR algorithm and how to use the trainer in a loop.

## Acceptance Criteria
- `cargo test -p pkr-cfr` passes.
- `Trainer` struct provides a clean, encapsulated interface for running CFR iterations.
