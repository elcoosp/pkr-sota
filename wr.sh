#!/usr/bin/env bash
set -euo pipefail
trap 'echo "ERROR on line $LINENO"; git checkout -- .; exit 1' ERR
DEBUG=${DEBUG:-0}; [ "$DEBUG" = "1" ] && set -x

WORKTREE_DIR="../pkr-sota-worktrees/task-W3-T4"
cd "$WORKTREE_DIR"

echo "=== Preparing updated PR description ==="

cat > /tmp/pr_body.md << 'EOF'
## W3-T4: VPS Memory Map Reader

### Summary
Implements `MmapReader` in `pkr-runtime`, a read‑only memory‑mapped parser for `blueprint.bin` files. The reader parses the file header using `bytemuck` and exposes raw byte slices for all sections (Fmph displacement data, translation table, CDFs) without any heap allocations for the blueprint data.

### Implementation
- **`crates/pkr-runtime/src/mmap.rs`** – Core module containing:
  - `MmapError` error enum (using `thiserror`) for file not found, invalid magic, unsupported version, truncated file, etc.
  - `MmapReader` struct that stores a `memmap2::Mmap`, a stable pointer to the parsed `FileHeader`, and pre‑computed offsets/lengths for each section.
  - Public methods: `file_header()`, `fmph_header()`, `fmph_data()`, `translation_table_header()`, `translation_table_data()`, `cdf_data()`.
  - Comprehensive unit tests (12 tests) that create valid blueprint files using `tempfile`, validate all accessors, and cover error conditions.
- **`crates/pkr-runtime/src/lib.rs`** – Re‑exports `MmapReader` and `MmapError`.
- **`crates/pkr-runtime/Cargo.toml`** – Added dependencies on `pkr-export` (for header structs) and `tempfile` (dev).

### Memory Footprint
The `MmapReader` owns only a `memmap2::Mmap` (which maps the file into the process address space) and a few `usize` offsets. No heap allocation is performed for the blueprint’s data sections – all access is via `&[u8]` slices backed by the mmap.

### Acceptance Criteria
- [x] `cargo test -p pkr-runtime` passes (12/12 tests).
- [x] Memory footprint equals file size (no heap copies of sections).
- [x] All file operations are read‑only (`File::open`, `Mmap::map`).
- [x] File‑not‑found errors handled gracefully (`MmapError::Io`).

### Test Results
```
running 12 tests
test mmap::tests::test_cdf_data_content ... ok
test mmap::tests::test_file_not_found ... ok
test mmap::tests::test_file_too_small ... ok
test mmap::tests::test_fmph_data_content ... ok
test mmap::tests::test_header_pointers_stable ... ok
test mmap::tests::test_invalid_magic ... ok
test mmap::tests::test_large_blueprint ... ok
test mmap::tests::test_open_valid_blueprint ... ok
test mmap::tests::test_translation_table_content ... ok
test mmap::tests::test_truncated_cdf_detected ... ok
test mmap::tests::test_unsupported_version ... ok
test mmap::tests::test_zero_length_sections ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```
EOF

echo "=== Updating PR #14 body ==="
gh pr edit 14 --body-file /tmp/pr_body.md

echo "=== Done. Verifying PR body ==="
gh pr view 14 --json body --jq '.body' | head -20
