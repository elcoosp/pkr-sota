#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
export RUSTFLAGS="-C target-cpu=native"
export RAYON_NUM_THREADS=8

OUT=outputs/v9
mkdir -p "$OUT"

cargo build --release -p pkr-trainer -p pkr-abstraction 2>&1 | tail -1

pre() {
    local target=""
    for arg in "$@"; do [[ "$arg" == *.bin ]] && target="$arg"; done
    if [ "${REBUILD:-0}" != "1" ] && [ -n "$target" ] && [ -s "$target" ]; then
        echo "  [cached] $1 -> $(basename "$target")"
        return 0
    fi
    cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- "$@"
}

echo "==> k=8 abstraction for v9"
pre hand_ranks "$OUT/hand_ranks.bin"
pre centroids 1000 8 "$OUT/hand_ranks.bin" "$OUT/centroids.bin"
pre flop "$OUT/hand_ranks.bin" "$OUT/flop_buckets.bin" 8
pre river "$OUT/hand_ranks.bin" "$OUT/river_buckets.bin" 8
pre preflop "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/preflop_abstraction.bin"
pre abs5 "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/abstraction.bin"

echo "==> turn table (45 min, one time)"
if [ -s "$OUT/turn_abstraction.bin" ]; then
    echo "  [cached] turn"
else
    EHS_SAMPLES=10 cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        turn "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/turn_abstraction.bin" 10000
fi

echo "==> TRAIN 100M iterations at k=8 (90 min)"
cargo run --release --quiet -p pkr-trainer -- \
    --iterations 100000000 \
    --threads 8 \
    --capacity 5000000 \
    --centroids "$OUT/centroids.bin" \
    --preflop-table "$OUT/preflop_abstraction.bin" \
    --flop-table "$OUT/abstraction.bin" \
    --flop-buckets "$OUT/flop_buckets.bin" \
    --turn-table "$OUT/turn_abstraction.bin" \
    --river-table "$OUT/river_buckets.bin" \
    --rank-table "$OUT/hand_ranks.bin" \
    --checkpoint "$OUT/train.ckpt" \
    --checkpoint-every 20000000 \
    --report-every 1000000 \
    --eval-every 5000000 \
    --eval-deals 500 \
    --output "$OUT/blueprint.bin" \
    --metrics-csv "$OUT/metrics.csv" \
    --stats-json "$OUT/stats.json"

echo "=== DONE ==="
