#!/bin/bash
set -euo pipefail
trap 'echo "ERROR on line $LINENO"; exit 1' ERR
DEBUG=${DEBUG:-0}; [ "$DEBUG" = "1" ] && set -x

WORKTREE_DIR="../pkr-sota-worktrees/task-W2-T4"
BRANCH="task/W2-T4"
cd "$WORKTREE_DIR"

# Push the latest commit
git push origin "$BRANCH" --force-with-lease

# Verify acceptance criteria one more time
echo "=== Final verification of W2-T4 ==="
cargo nextest run -p pkr-export
echo "All tests pass. Task W2-T4 is complete."
