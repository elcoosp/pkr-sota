#!/usr/bin/env bash
# trajectory.sh — dump CDFs for a run's chunks side by side
#
# 2026-11-03 re-audit (M12): this used `set -uo pipefail` (no -e) and
# hardcoded /tmp/v15_evals.log while the default VERSION moved to v16.
# Now -euo pipefail (the per-loop grep pipelines keep explicit || true
# so a chunk dump with no matching lines is not fatal) and the eval log
# path is derived from VERSION / EVAL_LOG like run.sh does.
set -euo pipefail
cd "$(dirname "$0")"

VERSION="${VERSION:-v16}"
OUT="${1:-outputs/${VERSION}}"
EVAL_LOG="${EVAL_LOG:-/tmp/${VERSION}_evals.log}"
if [ ! -d "$OUT" ]; then
    echo "no $OUT"
    exit 1
fi

for chunk in 5000000 10000000 15000000 20000000; do
    bp="$OUT/blueprint_${chunk}.bin"
    if [ ! -f "$bp" ]; then
        continue
    fi
    echo ""
    echo "--- $OUT @ $chunk ---"
    PKR_BLUEPRINT="$bp" PKR_ABS_DIR="$OUT" \
        cargo test --release -q -p pkr-trainer --test dump_cdf -- --ignored --nocapture 2>&1 \
        | grep -E "^  (AA|KK|QQ|99|AKs|T9s|72o|22|33) " | head -9 || true
done

echo ""
echo "=== evals ($EVAL_LOG) ==="
cat "$EVAL_LOG" 2>/dev/null || true
