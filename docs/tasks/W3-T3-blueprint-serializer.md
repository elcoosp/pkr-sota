# W3-T3: Blueprint Serializer

## Objective
Write the `blueprint.bin` file in `pkr-export` by combining the FMph, CDFs, and translation tables.

## Dependencies
- `pkr-contracts` (W0-T1)
- `pkr-cfr` (W1-T3)
- `pkr-export` (W1-T4, W2-T3, W2-T4)

## Exclusive File Paths
- `crates/pkr-export/src/writer.rs`
- `crates/pkr-export/src/lib.rs`

## TDD Instructions
1. **Red**: In `writer.rs`, write tests for `write_blueprint`. Create a mock `CompactRegretTable` and mock keys. Assert that `write_blueprint` creates a file, the file size matches expectations, and reading the header back yields the correct magic number and counts.
2. **Green**: Implement `write_blueprint(path: &str, table: &CompactRegretTable, keys: &[u64])`. Call `build_fmph` and `compute_translation`. Write the `FileHeader`, FMph data, CDF array (converted from regrets), and translation table sequentially to the file.
3. **Refactor**: Use `std::fs::File` and `std::io::Write`. Ensure all writes are aligned if necessary for `mmap`.

## Acceptance Criteria
- `cargo test -p pkr-export` passes.
- Output file is valid and ready for `pkr-runtime`.
