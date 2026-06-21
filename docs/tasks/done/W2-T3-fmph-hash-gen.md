# W2-T3: FMph Hash Gen

## Objective
Implement the Finite State Machine Minimal Perfect Hash (FMph) generation in `pkr-export`.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-export` (W1-T4)

## Exclusive File Paths
- `crates/pkr-export/src/fmph.rs`
- `crates/pkr-export/src/lib.rs`

## TDD Instructions
1. **Red**: In `fmph.rs`, write tests for `build_fmph`. Provide a `Vec<u64>` of 1000 random keys. Assert that the generated hash function maps every key to a unique index in `[0, 1000)` with zero collisions.
2. **Green**: Implement `build_fmph(keys: &[u64]) -> FmphData`. Use a simple algorithm like Hash, Displace, and Compress (or a basic lookup table approach). The output `FmphData` should contain the state needed to evaluate the hash.
3. **Refactor**: Optimize the build process. Ensure the `FmphData` struct is serializable with `bytemuck` or `serde`.

## Acceptance Criteria
- `cargo test -p pkr-export` passes.
- Zero collisions for the provided key set.
