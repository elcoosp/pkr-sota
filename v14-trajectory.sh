#!/usr/bin/env bash
# v14-trajectory.sh — wait for v14 to finish, then dump CDFs for all chunks
# and print a compact convergence table for the hands that matter.
set -uo pipefail
cd "$(dirname "$0")"

echo "waiting for v14 to finish..."
while pgrep -f 'pkr-trainer.*--iterations' >/dev/null; do
    sleep 20
done
echo "v14 done."

echo ""
echo "=== v14 chunk-by-chunk CDF trajectories ==="
echo ""
echo "Format: hand | fold | call | 0.4x | 0.8x | 1.6x | jam | argmax"
echo ""

# Extract only the six key hands from each dump.
for chunk in 5000000 10000000 15000000 20000000; do
    bp="outputs/v14/blueprint_${chunk}.bin"
    [ ! -f "$bp" ] && continue

    echo "--- v14 @ $chunk ---"
    PKR_BLUEPRINT="$bp" \
    PKR_ABS_DIR=outputs/v14 \
    cargo test --release -q -p pkr-trainer --test dump_cdf -- --ignored --nocapture 2>&1 \
        | grep -E "^  (AA|KK|QQ|99|AKs|T9s|72o|22|33) " \
        | head -9
    echo ""
done

echo ""
echo "=== v14 full eval log ==="
cat /tmp/v14_evals.log 2>/dev/null

echo ""
echo "=== final blueprint info ==="
ls -la outputs/v14/blueprint_*.bin 2>/dev/null
ls -la outputs/v14/train.ckpt 2>/dev/null
