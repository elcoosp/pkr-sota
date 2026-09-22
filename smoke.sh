#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

SMOKE_DIR="${SMOKE_DIR:-./outputs/v0-smoke}"
[ "${SMOKE_FRESH:-0}" = "1" ] && { echo "wiping $SMOKE_DIR"; rm -rf "$SMOKE_DIR"; }
mkdir -p "$SMOKE_DIR"
SMOKE_DIR_ABS="$(cd "$SMOKE_DIR" && pwd)"

echo "=== smoke test ==="
echo "dir: $SMOKE_DIR_ABS"
echo ""

# ensure TARGET CMD... — runs CMD unless TARGET exists and is non-empty.
# Propagates CMD's exit code (set -e makes the whole script fail on it).
ensure() {
    local target="$1"; shift
    if [ -s "$target" ]; then
        echo "  [cached] $(basename "$target")"
        return 0
    fi
    "$@"
}

pre() {
    cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- "$@"
}

echo "==> Building release binaries"
cargo build --release -p pkr-trainer -p pkr-abstraction 2>&1 | tail -1

echo "==> [1/8] hand_ranks.bin"
ensure "$SMOKE_DIR_ABS/hand_ranks.bin" \
    pre hand_ranks "$SMOKE_DIR_ABS/hand_ranks.bin"

echo "==> [2/8] centroids.bin"
ensure "$SMOKE_DIR_ABS/centroids.bin" \
    pre centroids 200 8 "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/centroids.bin"

echo "==> [3/8] preflop_abstraction.bin"
ensure "$SMOKE_DIR_ABS/preflop_abstraction.bin" \
    EHS_SAMPLES=20 pre preflop \
        "$SMOKE_DIR_ABS/centroids.bin" \
        "$SMOKE_DIR_ABS/hand_ranks.bin" \
        "$SMOKE_DIR_ABS/preflop_abstraction.bin"

echo "==> [4/8] flop_abstraction.bin"
ensure "$SMOKE_DIR_ABS/flop_abstraction.bin" \
    EHS_SAMPLES=5 pre abs5 \
        "$SMOKE_DIR_ABS/centroids.bin" \
        "$SMOKE_DIR_ABS/hand_ranks.bin" \
        "$SMOKE_DIR_ABS/flop_abstraction.bin"

echo "==> [5/8] flop_buckets.bin"
ensure "$SMOKE_DIR_ABS/flop_buckets.bin" \
    pre flop "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/flop_buckets.bin" 8

echo "==> [6/8] river_buckets.bin"
ensure "$SMOKE_DIR_ABS/river_buckets.bin" \
    EHS_SAMPLES=5 pre river \
        "$SMOKE_DIR_ABS/hand_ranks.bin" \
        "$SMOKE_DIR_ABS/river_buckets.bin" 8

echo "==> [7/8] turn_abstraction.bin (305 MB)"
ensure "$SMOKE_DIR_ABS/turn_abstraction.bin" \
    EHS_SAMPLES=1 pre turn \
        "$SMOKE_DIR_ABS/centroids.bin" \
        "$SMOKE_DIR_ABS/hand_ranks.bin" \
        "$SMOKE_DIR_ABS/turn_abstraction.bin" 100

echo "==> [8/8] pkr-trainer (10 iterations)"
EHS_SAMPLES=5 cargo run --release --quiet -p pkr-trainer -- \
    --iterations 10 \
    --threads 2 \
    --capacity 4096 \
    --centroids "$SMOKE_DIR_ABS/centroids.bin" \
    --preflop-table "$SMOKE_DIR_ABS/preflop_abstraction.bin" \
    --flop-table "$SMOKE_DIR_ABS/flop_abstraction.bin" \
    --flop-buckets "$SMOKE_DIR_ABS/flop_buckets.bin" \
    --turn-table "$SMOKE_DIR_ABS/turn_abstraction.bin" \
    --river-table "$SMOKE_DIR_ABS/river_buckets.bin" \
    --rank-table "$SMOKE_DIR_ABS/hand_ranks.bin" \
    --checkpoint "$SMOKE_DIR_ABS/train.ckpt" \
    --checkpoint-every 5 \
    --report-every 5 \
    --output "$SMOKE_DIR_ABS/blueprint.bin" \
    --metrics-csv "$SMOKE_DIR_ABS/metrics.csv" \
    --stats-json "$SMOKE_DIR_ABS/stats.json"

echo ""
echo "==> Validating outputs"
test -s "$SMOKE_DIR_ABS/blueprint.bin"   || { echo "FAIL: blueprint.bin"; exit 1; }
test -s "$SMOKE_DIR_ABS/train.ckpt"      || { echo "FAIL: train.ckpt"; exit 1; }
TURN_SIZE=$(wc -c < "$SMOKE_DIR_ABS/turn_abstraction.bin" | tr -d ' ')
[ "$TURN_SIZE" = "305377800" ] || { echo "FAIL: turn size $TURN_SIZE"; exit 1; }
RIVER_SIZE=$(wc -c < "$SMOKE_DIR_ABS/river_buckets.bin" | tr -d ' ')
[ "$RIVER_SIZE" = "2598960" ] || { echo "FAIL: river size $RIVER_SIZE"; exit 1; }
echo "  turn:  $TURN_SIZE bytes"
echo "  river: $RIVER_SIZE bytes"

echo ""
echo "==> Loading blueprint through pkr-runtime"
PKR_BLUEPRINT="$SMOKE_DIR_ABS/blueprint.bin" \
    cargo test --release --quiet -p pkr-trainer --test pipeline -- --ignored load_external_blueprint

echo ""
echo "=== smoke test passed ==="
