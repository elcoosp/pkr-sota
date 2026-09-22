#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

BENCH_DIR="${BENCH_DIR:-./.smoke}"
THREADS_LIST="${THREADS_LIST:-1 2 4 8}"
SECONDS_PER_RUN="${SECONDS_PER_RUN:-15}"

if [ ! -f "$BENCH_DIR/turn_abstraction.bin" ]; then
    echo "ERROR: $BENCH_DIR/turn_abstraction.bin not found."
    echo "Run ./smoke.sh first to generate the abstraction artifacts."
    exit 1
fi

BENCH_DIR_ABS="$(cd "$BENCH_DIR" && pwd)"

echo "=== pkr-sota throughput benchmark ==="
echo "  dir:        $BENCH_DIR_ABS"
echo "  threads:    $THREADS_LIST"
echo "  seconds:    $SECONDS_PER_RUN per config"
echo ""

cargo build --release -p pkr-trainer 2>&1 | tail -1

for T in $THREADS_LIST; do
    echo "--- threads=$T ---"
    # Allow this config to fail without killing the whole bench.
    set +e
    PKR_PHASE_PROFILE=1 cargo run --release -p pkr-trainer -- \
        --bench-seconds "$SECONDS_PER_RUN" \
        --threads "$T" \
        --capacity 50000000 \
        --centroids "$BENCH_DIR_ABS/centroids.bin" \
        --preflop-table "$BENCH_DIR_ABS/preflop_abstraction.bin" \
        --flop-table "$BENCH_DIR_ABS/flop_abstraction.bin" \
        --flop-buckets "$BENCH_DIR_ABS/flop_buckets.bin" \
        --turn-table "$BENCH_DIR_ABS/turn_abstraction.bin" \
        --river-table "$BENCH_DIR_ABS/river_buckets.bin" \
        --rank-table "$BENCH_DIR_ABS/hand_ranks.bin" \
        --output "$BENCH_DIR_ABS/bench_blueprint.bin" 2>&1 \
        | grep -E "(Running with|BENCH|iter .*infosets)"
    echo ""
    set -e
done

echo "=== benchmark complete ==="
echo "Compare the BENCH lines. it/s * 86400 = iterations per day."
echo "For reference: level-A arena-playable is roughly 1e6-1e7 iterations."
