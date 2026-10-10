#!/usr/bin/env bash
# Gate: no hard-coded action-bucket-count literals in src. K=7 drift (jam
# bucket 6 dropped) came from stale `.min(6)` / `[Option<usize>; 6]` /
# `b < 6` / `0..6` in decode paths. Use NUM_ACTION_BUCKETS instead.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
bad=$(grep -rnE "\.min\(6\)|\[Option<usize>; 6\]|b < 6\b|\(0\.\.6\)|0\.\.6\b" \
        crates binaries --include="*.rs" 2>/dev/null \
      | grep -v "test" | grep -v "NUM_ACTION_BUCKETS")
if [ -n "$bad" ]; then
  echo "FAIL: hard-coded 6-bucket literals (use NUM_ACTION_BUCKETS):"
  echo "$bad"
  exit 1
fi
echo "OK: no hard-coded action-bucket-count literals"
