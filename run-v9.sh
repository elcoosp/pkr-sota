#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

export RUSTFLAGS="-C target-cpu=native"
export RAYON_NUM_THREADS=8

OUT=outputs/v9
mkdir -p "$OUT"

# ---- Config ----------------------------------------------------------------
CENTROID_K=8
FLOP_BUCKETS=8
RIVER_BUCKETS=8
CAPACITY=5000000
ITERATIONS=100000000
CHECKPOINT_EVERY=20000000
REPORT_EVERY=1000000
EVAL_EVERY=5000000
EVAL_DEALS=500
EHS_SAMPLES=100
EHS_SAMPLES_TURN=10

export EHS_SAMPLES

cat > "$OUT/manifest.txt" << MANEOF
version=v9
centroid_k=$CENTROID_K
flop_buckets=$FLOP_BUCKETS
river_buckets=$RIVER_BUCKETS
capacity=$CAPACITY
iterations=$ITERATIONS
checkpoint_every=$CHECKPOINT_EVERY
eval_every=$EVAL_EVERY
eval_deals=$EVAL_DEALS
ehs_samples=$EHS_SAMPLES
ehs_samples_turn=$EHS_SAMPLES_TURN
started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
git_commit=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)
MANEOF

echo "=== pkr-sota k=8 training run ==="
echo "  version:    $OUT"
echo "  k:          $CENTROID_K"
echo "  capacity:   $CAPACITY"
echo "  iterations: $ITERATIONS"
echo "  eval:       every $EVAL_EVERY iters, $EVAL_DEALS deals"
echo ""

echo "==> building release binaries"
cargo build --release -p pkr-trainer -p pkr-abstraction 2>&1 | tail -1

# pre: cache precompute on the last .bin argument. Reuse if present.
pre() {
    local target=""
    for arg in "$@"; do
        [[ "$arg" == *.bin ]] && target="$arg"
    done
    if [ "${REBUILD:-0}" != "1" ] && [ -n "$target" ] && [ -s "$target" ]; then
        echo "  [cached] $1 -> $(basename "$target")"
        return 0
    fi
    cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- "$@"
}

echo "==> [1/7] hand_ranks"
pre hand_ranks "$OUT/hand_ranks.bin"

echo "==> [2/7] centroids (k=$CENTROID_K)"
pre centroids 1000 "$CENTROID_K" "$OUT/hand_ranks.bin" "$OUT/centroids.bin"

echo "==> [3/7] flop_buckets (k=$FLOP_BUCKETS)"
pre flop "$OUT/hand_ranks.bin" "$OUT/flop_buckets.bin" "$FLOP_BUCKETS"

echo "==> [4/7] river_buckets (k=$RIVER_BUCKETS)"
pre river "$OUT/hand_ranks.bin" "$OUT/river_buckets.bin" "$RIVER_BUCKETS"

echo "==> [5/7] preflop table"
pre preflop "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/preflop_abstraction.bin"

echo "==> [6/7] flop table"
pre abs5 "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/abstraction.bin"

echo "==> [7/7] turn table (305 MB; ~30-60 min if not cached)"
if [ -s "$OUT/turn_abstraction.bin" ]; then
    echo "  [cached] turn -> turn_abstraction.bin"
else
    EHS_SAMPLES="$EHS_SAMPLES_TURN" \
        cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        turn "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/turn_abstraction.bin" 10000
fi

echo ""
echo "==> TRAIN $ITERATIONS iterations at k=$CENTROID_K"
echo ""
cargo run --release --quiet -p pkr-trainer -- \
    --iterations "$ITERATIONS" \
    --threads 8 \
    --capacity "$CAPACITY" \
    --centroids "$OUT/centroids.bin" \
    --preflop-table "$OUT/preflop_abstraction.bin" \
    --flop-table "$OUT/abstraction.bin" \
    --flop-buckets "$OUT/flop_buckets.bin" \
    --turn-table "$OUT/turn_abstraction.bin" \
    --river-table "$OUT/river_buckets.bin" \
    --rank-table "$OUT/hand_ranks.bin" \
    --checkpoint "$OUT/train.ckpt" \
    --checkpoint-every "$CHECKPOINT_EVERY" \
    --report-every "$REPORT_EVERY" \
    --eval-every "$EVAL_EVERY" \
    --eval-deals "$EVAL_DEALS" \
    --output "$OUT/blueprint.bin" \
    --metrics-csv "$OUT/metrics.csv" \
    --stats-json "$OUT/stats.json"

echo ""
echo "=== DONE: $OUT/blueprint.bin ==="
echo "  metrics: $OUT/metrics.csv"
echo "  stats:   $OUT/stats.json"
echo "  eval:    grep '^EVAL' /tmp/v9.log"
