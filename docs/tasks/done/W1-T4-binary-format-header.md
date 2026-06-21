# W1-T4: Binary Format Header

## Objective
Define the `#[repr(C)]` structs for the `blueprint.bin` memory-mapped file in `pkr-export`.

## Dependencies
- `pkr-contracts` (W0-T1)

## Exclusive File Paths
- `crates/pkr-export/Cargo.toml`
- `crates/pkr-export/src/header.rs`
- `crates/pkr-export/src/lib.rs`

## TDD Instructions
1. **Red**: In `header.rs`, write tests verifying that `FileHeader`, `FmphHeader`, and `TranslationTableHeader` have the correct sizes and alignments using `std::mem::size_of` and `bytemuck::Pod`.
2. **Green**: Define the structs. `FileHeader` must contain a magic number (e.g., `*b"PKRSOTA1"`), version, variant ID, max actions K, infoset count. Derive `Pod`, `Zeroable` from `bytemuck`.
3. **Refactor**: Ensure no padding issues by explicitly sizing fields (e.g., `u32`, `u64`).

## Acceptance Criteria
- `cargo test -p pkr-export` passes.
- Structs are safe for zero-copy casting.
