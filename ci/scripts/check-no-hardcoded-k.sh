#!/usr/bin/env bash
# Gate: no hard-coded action-bucket-count literals in src, and no stale
# "6-bucket" doc references after the K=7 flip (NUM_ACTION_BUCKETS = 7).
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1

fail=0

# 1. Code literals: hard-coded 6 where NUM_ACTION_BUCKETS should be used.
bad_code=$(grep -rnE "\.min\(6\)|\[Option<usize>; 6\]|b < 6\b|\(0\.\.6\)|0\.\.6\b" \
        crates binaries --include="*.rs" 2>/dev/null \
      | grep -v "test" | grep -v "NUM_ACTION_BUCKETS")
if [ -n "$bad_code" ]; then
  echo "FAIL: hard-coded 6-bucket literals (use NUM_ACTION_BUCKETS):"
  echo "$bad_code"
  fail=1
fi

# 2. Doc phrase: "6-bucket" outside frozen historical/audit docs.
bad_doc=$(grep -rn "6-bucket" crates binaries docs \
        --include="*.rs" --include="*.md" 2>/dev/null \
      | grep -v "/target/" \
      | grep -v "^docs/PKR_AUDIT_AND_FIX_PLAN.md:" \
      | grep -v "^docs/reviews/" \
      | grep -v "^docs/experiments/k7-landed-review-fixes.md:" \
      | grep -v "^docs/experiments/k6-vs-k7-incomparable.md:" \
      | grep -v "^docs/handoffs/")
if [ -n "$bad_doc" ]; then
  echo "FAIL: stale '6-bucket' doc/comment references (K=7 now):"
  echo "$bad_doc"
  fail=1
fi

if [ "$fail" = 0 ]; then
  echo "OK: no hard-coded bucket-count literals or stale 6-bucket refs"
fi
exit "$fail"
