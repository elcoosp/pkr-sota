#!/usr/bin/env bash
set -euo pipefail
trap 'echo "ERROR on line $LINENO"; git checkout -- .; exit 1' ERR
DEBUG=${DEBUG:-0}; [ "$DEBUG" = "1" ] && set -x

WORKTREE_DIR="../pkr-sota-worktrees/task-W4-T3"
BRANCH="task/W4-T3"
cd "$WORKTREE_DIR"

# Get the PR number for this branch
PR_NUMBER=$(gh pr view "$BRANCH" --json number -q '.number')
echo "==> Updating PR #$PR_NUMBER description"

# Structured markdown body
NEW_BODY=$(cat <<'PRBODY'
## Objective
Implement the fast‑path runtime lookup engine in `pkr-runtime` (W4-T3).
The `SolverHandle` wraps the memory‑mapped blueprint and performs O(1)
Minimal Perfect Hash (MPH) lookups to retrieve strategy CDFs.

## Changes
- **`crates/pkr-runtime/src/lookup.rs`** (new file)
  - `SolverHandle` struct wrapping `MmapReader`
  - `get_advice_fast(infoset_hash) -> Option<SotaAdvice>`
  - Implements `pkr_contracts::BlueprintProvider`
  - Single‑level Hash‑and‑Displace MPH evaluation (`eval_mph`)
- **`crates/pkr-runtime/src/lib.rs`**
  - Added `pub mod lookup;`

## Tests
| Test | Status |
|------|--------|
| `basic_lookup_returns_correct_cdf` | ✅ |
| `empty_blueprint_returns_none` | ✅ |
| `implements_blueprint_provider` | ✅ |
| `deterministic_output_for_same_key` | ✅ |
| `index_out_of_bounds_returns_none` | ✅ |
| `large_keyset_stress_test` | ✅ |
| `mph_no_collisions` | ✅ |
| `blueprint_provider_trait_object_send_sync` | ✅ |
| `hash_key_deterministic_and_no_panic` | ✅ |
| All existing `mmap` tests | ✅ (12 tests) |

**Total: 21 tests passed**

## Acceptance Criteria
- [x] `cargo test -p pkr-runtime` passes (all 21 tests)
- [x] Lookup is O(1) branchless (excluding final bounds check)
- [x] Implements `pkr_contracts::BlueprintProvider`
- [x] No merge conflicts – only allowed files touched

## Notes
- MPH evaluation matches the single‑level format produced by `pkr-export::fmph`
- Displacement data length forced to a multiple of 8 bytes to satisfy alignment requirements of `TranslationTableHeader`
PRBODY
)

gh pr edit "$PR_NUMBER" --body "$NEW_BODY"
echo "==> PR description updated"
