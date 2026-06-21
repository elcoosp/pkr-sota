# W0-T1: Contracts Crate API Boundary

## Objective
Define the universal traits and structs in `pkr-contracts` that all other crates will code against. This crate must have zero internal dependencies.

## Exclusive File Paths
- `crates/pkr-contracts/Cargo.toml`
- `crates/pkr-contracts/src/lib.rs`

## Dependencies
None.

## TDD Instructions
1. **Red**: Write a test in `src/lib.rs` (using `#[cfg(test)]`) that attempts to instantiate mock structs implementing `GameRules`, `Evaluator`, `AbstractionBuilder`, and `BlueprintProvider`. Assert that the traits have the required method signatures.
2. **Green**: Implement the traits and structs: `GameRules`, `InfoSet`, `SotaAdvice`, `BlueprintProvider`, `Evaluator`, `AbstractionBuilder`. Use `u64` for hashes, `Vec<u8>` for CDFs.
3. **Refactor**: Ensure `Send + Sync` bounds are applied to all traits. Document all public items.

## Acceptance Criteria
- `cargo test -p pkr-contracts` passes.
- All traits are public and documented.
- `Cargo.toml` only depends on `serde` and `bytemuck`.
