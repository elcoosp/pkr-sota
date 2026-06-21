# W4-T3: Runtime Lookup Engine

## Objective
Implement the fast-path runtime lookup engine in `pkr-runtime` that executes the FMph hash and returns the strategy.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-runtime` (W3-T4)

## Exclusive File Paths
- `crates/pkr-runtime/src/lookup.rs`
- `crates/pkr-runtime/src/lib.rs`

## TDD Instructions
1. **Red**: In `lookup.rs`, write tests for `SolverHandle::get_advice_fast`. Create a mock `MmapReader` with dummy FMph and CDF data. Assert that `get_advice_fast` correctly executes the hash, reads the `u8` CDF values, and returns an `SotaAdvice` struct with the correct probabilities.
2. **Green**: Implement `SolverHandle` which wraps the `MmapReader`. Implement `SolverHandle::new(mmap_reader: MmapReader) -> Self`. Implement `get_advice_fast(&self, infoset_hash: u64) -> Option<SotaAdvice>`. Perform the FMph lookup using the memory-mapped bytes, extract the 4 bytes corresponding to the CDF, and return them.
3. **Refactor**: Ensure the lookup function is `O(1)` and branchless where possible (excluding the final bounds check). Verify it implements `pkr_contracts::BlueprintProvider`.

## Acceptance Criteria
- `cargo test -p pkr-runtime` passes.
- Lookup execution time is sub-microsecond (verifiable via `cargo bench` if set up, but functionally correct via tests).
- Implements `pkr_contracts::BlueprintProvider`.
