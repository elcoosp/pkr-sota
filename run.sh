#!/usr/bin/env bash
set -euo pipefail
export RUSTFLAGS="-C target-cpu=native"

VERSION="${VERSION:-v1}"
OUT="outputs/${VERSION}"
mkdir -p "$OUT"

THREADS="${THREADS:-8}"
ITERATIONS="${ITERATIONS:-1000000}"
CENTROID_SAMPLES="${CENTROID_SAMPLES:-1000}"
CENTROID_K="${CENTROID_K:-200}"
FLOP_BUCKETS="${FLOP_BUCKETS:-200}"
RIVER_BUCKETS="${RIVER_BUCKETS:-200}"
EHS_SAMPLES="${EHS_SAMPLES:-100}"
EHS_SAMPLES_TURN="${EHS_SAMPLES_TURN:-10}"
CAPACITY="${CAPACITY:-50000000}"
CHECKPOINT_EVERY="${CHECKPOINT_EVERY:-20000000}"

export RAYON_NUM_THREADS="$THREADS" EHS_SAMPLES

echo "=== pkr-sota TRAINING PIPELINE ==="
echo "Version: $VERSION ($OUT/) | Threads: $THREADS | Iters: $ITERATIONS | k=$CENTROID_K"

cat > "$OUT/manifest.txt" << MANEOF
version=$VERSION
threads=$THREADS
iterations=$ITERATIONS
centroid_samples=$CENTROID_SAMPLES
centroid_k=$CENTROID_K
flop_buckets=$FLOP_BUCKETS
river_buckets=$RIVER_BUCKETS
ehs_samples=$EHS_SAMPLES
ehs_samples_turn=$EHS_SAMPLES_TURN
capacity=$CAPACITY
started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
git_commit=$(git rev-parse --short HEAD 2>/dev/null || echo unknown)
MANEOF

cargo build --release -p pkr-trainer -p pkr-abstraction

pre() { cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- "$@"; }

echo "==> [1/8] hand_ranks"; pre hand_ranks "$OUT/hand_ranks.bin"
echo "==> [2/8] centroids"; pre centroids "$CENTROID_SAMPLES" "$CENTROID_K" "$OUT/hand_ranks.bin" "$OUT/centroids.bin"
echo "==> [3/8] flop_buckets"; pre flop "$OUT/hand_ranks.bin" "$OUT/flop_buckets.bin" "$FLOP_BUCKETS"
echo "==> [4/8] river_buckets"; pre river "$OUT/hand_ranks.bin" "$OUT/river_buckets.bin" "$RIVER_BUCKETS"
echo "==> [5/8] preflop table"; pre preflop "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/preflop_abstraction.bin"
echo "==> [6/8] flop table"; pre abs5 "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/abstraction.bin"
echo "==> [7/8] turn table"; EHS_SAMPLES="$EHS_SAMPLES_TURN" pre turn "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/turn_abstraction.bin" 10000

echo "==> [8/8] TRAIN"
cargo run --release -p pkr-trainer -- \
    --iterations "$ITERATIONS" --threads "$THREADS" --capacity "$CAPACITY" \
    --centroids "$OUT/centroids.bin" \
    --preflop-table "$OUT/preflop_abstraction.bin" \
    --flop-table "$OUT/abstraction.bin" \
    --flop-buckets "$OUT/flop_buckets.bin" \
    --turn-table "$OUT/turn_abstraction.bin" \
    --river-table "$OUT/river_buckets.bin" \
    --rank-table "$OUT/hand_ranks.bin" \
    --checkpoint "$OUT/train.ckpt" --checkpoint-every "$CHECKPOINT_EVERY" \
    --report-every 10000 \
    --output "$OUT/blueprint.bin" \
    --metrics-csv "$OUT/metrics.csv" --stats-json "$OUT/stats.json"

echo "=== DONE: $OUT/blueprint.bin ==="
