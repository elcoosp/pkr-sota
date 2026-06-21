#!/usr/bin/env bash
set -euo pipefail
trap 'echo "ERROR on line $LINENO"; exit 1' ERR
DEBUG=${DEBUG:-0}; [ "$DEBUG" = "1" ] && set -x

WORKTREE_DIR="../pkr-sota-worktrees/task-W1-T4"
BRANCH="task/W1-T4"

if [ -d "$WORKTREE_DIR" ]; then
    cd "$WORKTREE_DIR"
else
    echo "Worktree not found, run the first script again."
    exit 1
fi

# Structured PR body in markdown
read -r -d '' BODY << 'PRBODY' || true
## W1-T4: Binary Format Header

### Objective
Define `#[repr(C)]` structs for the `blueprint.bin` memory‑mapped file in `pkr-export`, compatible with zero‑copy casting via `bytemuck`.

### Changes
- **`crates/pkr-export/Cargo.toml`** – added `bytemuck` workspace dependency.
- **`crates/pkr-export/src/lib.rs`** – declared `pub mod header;`.
- **`crates/pkr-export/src/header.rs`** – introduced three structs:
  - `FileHeader` – magic (`PKRSOTA1`), version, variant ID, infoset count, max actions K, with explicit padding to 32 bytes.
  - `FmphHeader` – Fmph key count, seed, max level size, level count, with explicit padding to 32 bytes.
  - `TranslationTableHeader` – number of entries, action size, with explicit padding to 16 bytes.
  All structs derive `Pod` and `Zeroable` for safe zero‑copy casting.

### Testing
- 6 original tests verify sizes, alignments, magic, `Pod`/`Zeroable` trait satisfaction, and zeroed state.
- Extended with:
  - Round‑trip tests via `bytemuck::bytes_of` and `from_bytes`.
  - Slice casting (`cast_slice`) for all three header types.
  - Zero‑padding verification.
  - Alignment checks within arrays.
  - Offset test confirming magic is at byte 0.
- All tests pass (`cargo test -p pkr-export` ✅).

### Acceptance Criteria
- [x] `cargo test -p pkr-export` passes.
- [x] Structs are safe for zero‑copy casting (derive `Pod`, `Zeroable`).
- [x] No implicit padding (fields are explicitly sized and padded).
PRBODY

echo "--- Updating PR description ---"
gh pr edit --body "$BODY"

echo "--- PR description updated. ---"
