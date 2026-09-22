#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

PROF_DIR="${PROF_DIR:-./.proftest}"
mkdir -p "$PROF_DIR"
PROF_DIR_ABS="$(cd "$PROF_DIR" && pwd)"

ITERATIONS="${ITERATIONS:-100000}"
THREADS="${THREADS:-8}"
CAPACITY="${CAPACITY:-50000000}"

echo "=== pkr-sota production-scale profile ==="
echo "  iterations:  $ITERATIONS"
echo "  threads:     $THREADS"
echo "  capacity:    $CAPACITY"
echo "  dir:         $PROF_DIR_ABS"
echo ""

echo "==> Building release binaries..."
cargo build --release -p pkr-trainer -p pkr-abstraction 2>&1 | tail -1

if [ ! -f "$PROF_DIR_ABS/hand_ranks.bin" ]; then
    echo "==> [1/4] hand_ranks.bin (~1-2 min)"
    cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        hand_ranks "$PROF_DIR_ABS/hand_ranks.bin"
else
    echo "==> [1/4] hand_ranks.bin (cached)"
fi

if [ ! -f "$PROF_DIR_ABS/centroids.bin" ]; then
    echo "==> [2/4] centroids.bin (k=64, samples=1000)"
    cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        centroids 1000 64 "$PROF_DIR_ABS/hand_ranks.bin" "$PROF_DIR_ABS/centroids.bin"
else
    echo "==> [2/4] centroids.bin (cached)"
fi

if [ ! -f "$PROF_DIR_ABS/preflop_abstraction.bin" ]; then
    echo "==> [3/4] preflop_abstraction.bin"
    EHS_SAMPLES=20 cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        preflop "$PROF_DIR_ABS/centroids.bin" "$PROF_DIR_ABS/hand_ranks.bin" \
        "$PROF_DIR_ABS/preflop_abstraction.bin"
else
    echo "==> [3/4] preflop_abstraction.bin (cached)"
fi

if [ ! -f "$PROF_DIR_ABS/flop_abstraction.bin" ]; then
    echo "==> [4/4] flop_abstraction.bin"
    EHS_SAMPLES=5 cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        abs5 "$PROF_DIR_ABS/centroids.bin" "$PROF_DIR_ABS/hand_ranks.bin" \
        "$PROF_DIR_ABS/flop_abstraction.bin"
else
    echo "==> [4/4] flop_abstraction.bin (cached)"
fi

echo ""
echo "==> Training $ITERATIONS iterations with full instrumentation..."
time cargo run --release -p pkr-trainer -- \
    --iterations "$ITERATIONS" \
    --threads "$THREADS" \
    --capacity "$CAPACITY" \
    --checkpoint-every 0 \
    --report-every 5000 \
    --centroids "$PROF_DIR_ABS/centroids.bin" \
    --preflop-table "$PROF_DIR_ABS/preflop_abstraction.bin" \
    --flop-table "$PROF_DIR_ABS/flop_abstraction.bin" \
    --rank-table "$PROF_DIR_ABS/hand_ranks.bin" \
    --output "$PROF_DIR_ABS/blueprint.bin" \
    --metrics-csv "$PROF_DIR_ABS/metrics.csv" \
    --stats-json "$PROF_DIR_ABS/stats.json"

echo ""
echo "=== Validating JSON output ==="
if python3 -c "import json; json.load(open('$PROF_DIR_ABS/stats.json'))" 2>/dev/null; then
    echo "  stats.json is valid JSON"
else
    echo "  ERROR: stats.json is not valid JSON"
    exit 1
fi

echo ""
echo "=== Artifacts ==="
ls -la "$PROF_DIR_ABS" | grep -E "(metrics\.csv|stats\.json|blueprint\.bin)"
echo ""
echo "Hand these to another AI for analysis:"
echo "  $PROF_DIR_ABS/metrics.csv"
echo "  $PROF_DIR_ABS/stats.json"
echo "  $PROF_DIR_ABS/blueprint.bin"
