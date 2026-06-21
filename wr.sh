#!/usr/bin/env bash
set -euo pipefail
trap 'echo "ERROR on line $LINENO"; git checkout -- .; exit 1' ERR
DEBUG=${DEBUG:-0}; [ "$DEBUG" = "1" ] && set -x

# ─── Worktree setup (optional, just to have the repo context) ──
WORKTREE_DIR="../pkr-sota-worktrees/task-W2-T1"
BRANCH="task/W2-T1"
mkdir -p ../pkr-sota-worktrees
if [ -d "$WORKTREE_DIR" ]; then
    cd "$WORKTREE_DIR"
else
    echo "Worktree directory missing – run the first script to create it."
    exit 1
fi

# ─── Update PR #7 description ─────────────────────────────
gh pr edit 7 --body '
## Task W2-T1: DCFR Math

### Overview
Implements the Discounted CFR (DCFR) regret update function in `pkr-cfr/src/dcfr.rs`,
following Brown & Sandholm (2019) with α=1.5 for positive regrets and α=0.0 for negative.

### Implementation
- `update_regret(current: u8, iteration: u32, delta: f32, is_positive: bool) -> u8`
- Discount factor: `t^α / (t^α + 1)`
  - **Positive regrets**: α = 1.5 → factor grows from 0 to ~1 as t increases.
  - **Negative regrets**: α = 0.0 → constant factor = 0.5 for t > 0.
- Edge case `t=0` handled explicitly (factor=0).
- Result clamped to `[0, 255]` via `i32::clamp`.

### Tests (58 passed)
**Core formula correctness**
- Positive/negative discount factors computed manually.
- t=0, t=1 special cases.
- Delta added after discount.

**Clamping & range**
- Values clamped to 0–255.
- Brute-force scan across multiple inputs never panics.

**Monotonicity & limits**
- Positive factor monotonic with iteration.
- Negative factor constant 0.5 for t>0.
- Large t (u32::MAX) factor → 1.0 for positive.

**Edge cases & robustness**
- NaN, ±∞, f32::MAX, f32::MIN delta values.
- No floating-point panics.

### Acceptance Criteria
- [x] `cargo test -p pkr-cfr` passes (58 tests).
- [x] `cargo clippy -p pkr-cfr -- -D warnings` clean.
- [x] DCFR formula matches Brown & Sandholm (2019) with α=1.5, β=0.
- [x] No unsafe code; safe zero-copy casting not required here.

### Commits
- `feat(cfr): implement DCFR update_regret (W2-T1)`
- `test(cfr): add more tests for DCFR update_regret`
- `fix(cfr): remove useless comparison to silence clippy warning`
- `test(cfr): finalise DCFR tests with NaN, infinity, and edge cases`
'
echo "PR #7 description updated successfully."
