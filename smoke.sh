#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

SMOKE_DIR="${SMOKE_DIR:-./.smoke}"
SMOKE_FRESH="${SMOKE_FRESH:-0}"

mkdir -p "$SMOKE_DIR"
SMOKE_DIR_ABS="$(cd "$SMOKE_DIR" && pwd)"

if [ "$SMOKE_FRESH" = "1" ]; then
    echo "=== SMOKE_FRESH=1: wiping $SMOKE_DIR ==="
    rm -rf "$SMOKE_DIR"
    mkdir -p "$SMOKE_DIR"
fi

echo "=== pkr-sota smoke test ==="
echo "Work dir: $SMOKE_DIR"
echo "Tip: SMOKE_FRESH=1 forces full regeneration."
echo ""

# Helper: run a command only if the target file doesn't exist or is empty.
need() {
    local target="$1"; shift
    if [ -s "$target" ]; then
        echo "  [cached] $(basename "$target")"
        return 1
    fi
    "$@"
    return 0
}

# -------------------------------------------------------------------------
echo "==> Building release binaries (cached by cargo)"
cargo build --release -p pkr-trainer -p pkr-abstraction 2>&1 | tail -1

# -------------------------------------------------------------------------
echo "==> [1/8] hand_ranks.bin"
if ! need "$SMOKE_DIR_ABS/hand_ranks.bin" \
    cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        hand_ranks "$SMOKE_DIR_ABS/hand_ranks.bin"; then :; fi

# -------------------------------------------------------------------------
echo "==> [2/8] centroids.bin (k=8, samples=200)"
if ! need "$SMOKE_DIR_ABS/centroids.bin" \
    cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        centroids 200 8 "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/centroids.bin"; then :; fi

# -------------------------------------------------------------------------
echo "==> [3/8] preflop_abstraction.bin"
if ! need "$SMOKE_DIR_ABS/preflop_abstraction.bin" \
    env EHS_SAMPLES=20 cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        preflop "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" \
        "$SMOKE_DIR_ABS/preflop_abstraction.bin"; then :; fi

# -------------------------------------------------------------------------
echo "==> [4/8] flop_abstraction.bin (abs5)"
if ! need "$SMOKE_DIR_ABS/flop_abstraction.bin" \
    env EHS_SAMPLES=5 cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        abs5 "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" \
        "$SMOKE_DIR_ABS/flop_abstraction.bin"; then :; fi

# -------------------------------------------------------------------------
echo "==> [5/8] flop_buckets.bin (k=8)"
if ! need "$SMOKE_DIR_ABS/flop_buckets.bin" \
    cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        flop "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/flop_buckets.bin" 8; then :; fi

# -------------------------------------------------------------------------
echo "==> [6/8] river_buckets.bin (k=8)"
if ! need "$SMOKE_DIR_ABS/river_buckets.bin" \
    env EHS_SAMPLES=5 cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        river "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/river_buckets.bin" 8; then :; fi

# -------------------------------------------------------------------------
echo "==> [7/8] turn_abstraction.bin (305MB)"
if ! need "$SMOKE_DIR_ABS/turn_abstraction.bin" \
    env EHS_SAMPLES=1 cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        turn "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" \
        "$SMOKE_DIR_ABS/turn_abstraction.bin" 100; then :; fi

# -------------------------------------------------------------------------
echo "==> [8/8] pkr-trainer (10 iterations)"
# Always rerun training: it is <1s and it exercises the current code.
env EHS_SAMPLES=5 cargo run --release --quiet -p pkr-trainer -- \
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
echo "==> Checking outputs"
test -s "$SMOKE_DIR_ABS/blueprint.bin" || { echo "FAIL: blueprint.bin missing"; exit 1; }
test -s "$SMOKE_DIR_ABS/train.ckpt" || { echo "FAIL: train.ckpt missing"; exit 1; }

TURN_SIZE=$(wc -c < "$SMOKE_DIR_ABS/turn_abstraction.bin" | tr -d ' ')
if [ "$TURN_SIZE" != "305377800" ]; then
    echo "FAIL: turn_abstraction.bin size $TURN_SIZE (expected 305377800)"
    exit 1
fi
RIVER_SIZE=$(wc -c < "$SMOKE_DIR_ABS/river_buckets.bin" | tr -d ' ')
if [ "$RIVER_SIZE" != "2598960" ]; then
    echo "FAIL: river_buckets.bin size $RIVER_SIZE (expected 2598960)"
    exit 1
fi
echo "  turn_abstraction.bin: $TURN_SIZE bytes"
echo "  river_buckets.bin:    $RIVER_SIZE bytes"

echo ""
echo "==> Loading the produced blueprint through pkr-runtime"
PKR_BLUEPRINT="$SMOKE_DIR_ABS/blueprint.bin" \
    cargo test --release --quiet -p pkr-trainer --test pipeline -- --ignored load_external_blueprint

echo ""
echo "=== smoke test passed ==="
