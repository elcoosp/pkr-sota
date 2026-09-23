#!/usr/bin/env bash
# trajectory.sh — dump CDFs for a run's chunks side by side
set -uo pipefail
cd "$(dirname "$0")"

OUT="${1:-outputs/v15}"
[ ! -d "$OUT" ] && { echo "no $OUT"; exit 1; }

for chunk in 5000000 10000000 15000000 20000000; do
    bp="$OUT/blueprint_${chunk}.bin"
    [ ! -f "$bp" ] && continue
    echo ""
    echo "--- $OUT @ $chunk ---"
    PKR_BLUEPRINT="$bp" PKR_ABS_DIR="$OUT" \
        cargo test --release -q -p pkr-trainer --test dump_cdf -- --ignored --nocapture 2>&1 \
        | grep -E "^  (AA|KK|QQ|99|AKs|T9s|72o|22|33) " | head -9
done

echo ""
echo "=== evals ==="
cat /tmp/v15_evals.log 2>/dev/null
