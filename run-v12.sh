#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"

echo "===== 1. diff summary (added/removed per file) ====="
git diff --numstat

echo ""
echo "===== 2. cargo fmt --check (is this just formatting?) ====="
cargo fmt --check --all 2>&1 | head -40
FMT_RC=${PIPESTATUS[0]}
echo "  fmt --check rc=$FMT_RC  (0 = nothing to format; nonzero = files above)"

echo ""
echo "===== 3. does the working tree compile? ====="
cargo check --workspace 2>&1 | tail -15
echo "  check rc=${PIPESTATUS[0]}"

echo ""
echo "===== 4. sample actual diffs (first 40 lines each) ====="
for f in \
    crates/pkr-cfr/src/dcfr.rs \
    crates/pkr-cfr/src/table.rs \
    crates/pkr-cfr/src/preflop_validate.rs \
    crates/pkr-abstraction/src/lib.rs \
    crates/pkr-eval/src/lookup_fast.rs \
    crates/pkr-exploit/src/best_response.rs \
    crates/pkr-export/src/writer.rs \
    crates/pkr-runtime/src/mmap.rs \
    crates/pkr-runtime/src/translate.rs \
    crates/pkr-fuzz/src/lib.rs \
    binaries/pkr-trainer/tests/eval_harness.rs \
    crates/pkr-runtime/tests/roundtrip.rs ; do
    echo ""
    echo "----- $f -----"
    git diff -- "$f" | head -40
done

echo ""
echo "===== 5. does HEAD alone (without these changes) still compile? ====="
echo "(using a scratch worktree so your current tree is untouched)"
SCRATCH=/tmp/pkr-head-clean
rm -rf "$SCRATCH"
if git worktree add --detach "$SCRATCH" 1d64b51 >/dev/null 2>&1; then
    ( cd "$SCRATCH" && cargo check --workspace 2>&1 | tail -8 )
    echo "  head-only check rc=$?"
    git worktree remove --force "$SCRATCH" >/dev/null 2>&1 || true
else
    echo "  (worktree add failed; skipping)"
fi

echo ""
echo "===== DONE ====="
