#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

SMOKE_DIR="${SMOKE_DIR:-./.smoke}"

echo "=== pkr-sota smoke test ==="
echo "Work dir: $SMOKE_DIR"
echo "Runs the real CLI end-to-end with tiny params, including turn and river"
echo "abstraction tables. Uses EHS_SAMPLES=1 for the turn precompute so the"
echo "305 MB table builds in seconds instead of hours."

rm -rf "$SMOKE_DIR"
mkdir -p "$SMOKE_DIR"

SMOKE_DIR_ABS="$(cd "$SMOKE_DIR" && pwd)"

echo "==> [1/8] hand_ranks.bin"
cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    hand_ranks "$SMOKE_DIR_ABS/hand_ranks.bin"

echo "==> [2/8] centroids.bin (k=8, samples=200)"
cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    centroids 200 8 "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/centroids.bin"

echo "==> [3/8] preflop_abstraction.bin"
EHS_SAMPLES=20 cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    preflop "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" \
    "$SMOKE_DIR_ABS/preflop_abstraction.bin"

echo "==> [4/8] flop abstraction table"
EHS_SAMPLES=5 cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    abs5 "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" \
    "$SMOKE_DIR_ABS/flop_abstraction.bin"

echo "==> [5/8] flop buckets"
cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    flop "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/flop_buckets.bin" 8

echo "==> [6/8] river board buckets (k=8)"
EHS_SAMPLES=5 cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    river "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/river_buckets.bin" 8

echo "==> [7/8] turn abstraction table (305 MB, EHS_SAMPLES=1)"
EHS_SAMPLES=1 cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    turn "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" \
    "$SMOKE_DIR_ABS/turn_abstraction.bin" 100

echo "==> [8/8] pkr-trainer (10 iterations, capacity=4096, with turn+river tables)"
EHS_SAMPLES=5 cargo run --release -p pkr-trainer -- \
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
    --output "$SMOKE_DIR_ABS/blueprint.bin"

echo "==> Checking outputs exist and are non-empty"
test -s "$SMOKE_DIR_ABS/blueprint.bin" || { echo "FAIL: missing or empty blueprint"; exit 1; }
SIZE=$(wc -c < "$SMOKE_DIR_ABS/blueprint.bin" | tr -d ' ')
echo "blueprint.bin: $SIZE bytes"

test -s "$SMOKE_DIR_ABS/train.ckpt" || { echo "FAIL: checkpoint not written"; exit 1; }
CKPT_SIZE=$(wc -c < "$SMOKE_DIR_ABS/train.ckpt" | tr -d ' ')
echo "train.ckpt: $CKPT_SIZE bytes"

TURN_SIZE=$(wc -c < "$SMOKE_DIR_ABS/turn_abstraction.bin" | tr -d ' ')
echo "turn_abstraction.bin: $TURN_SIZE bytes (expected 305377800)"
if [ "$TURN_SIZE" != "305377800" ]; then
    echo "FAIL: turn table has unexpected size"
    exit 1
fi

RIVER_SIZE=$(wc -c < "$SMOKE_DIR_ABS/river_buckets.bin" | tr -d ' ')
echo "river_buckets.bin: $RIVER_SIZE bytes (expected 2598960)"
if [ "$RIVER_SIZE" != "2598960" ]; then
    echo "FAIL: river table has unexpected size"
    exit 1
fi

echo "==> Loading the produced blueprint through pkr-runtime"
PKR_BLUEPRINT="$SMOKE_DIR_ABS/blueprint.bin" \
    cargo test --release -p pkr-trainer --test pipeline -- --ignored load_external_blueprint

echo "=== smoke test passed ==="
