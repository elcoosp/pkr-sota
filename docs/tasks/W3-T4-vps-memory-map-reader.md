# W3-T4: VPS Memory Map Reader

## Objective
Implement the read-only memory mapper in `pkr-runtime` to parse `blueprint.bin`.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-export` (W1-T4 - for header struct definitions)

## Exclusive File Paths
- `crates/pkr-runtime/Cargo.toml`
- `crates/pkr-runtime/src/mmap.rs`
- `crates/pkr-runtime/src/lib.rs`

## TDD Instructions
1. **Red**: In `mmap.rs`, write tests for `MmapReader::new`. Create a dummy binary file matching the `FileHeader` layout. Assert that `MmapReader` opens it, parses the header, and provides correct slices/pointers to the data sections.
2. **Green**: Implement `MmapReader`. Use `memmap2::Mmap`. Parse the `FileHeader` using `bytemuck::from_bytes`. Calculate offsets for FMph, CDFs, and Translation Table based on header counts. Provide methods to get raw byte slices for each section.
3. **Refactor**: Ensure all file operations are read-only. Handle file not found errors gracefully with `thiserror`.

## Acceptance Criteria
- `cargo test -p pkr-runtime` passes.
- Memory footprint is exactly the size of the file (no heap allocation for the blueprint itself).
