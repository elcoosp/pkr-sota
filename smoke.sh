#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

SMOKE_DIR="${SMOKE_DIR:-./.smoke}"

echo "=== pkr-sota smoke test ==="
echo "Work dir: $SMOKE_DIR"
echo "This runs the real CLI end-to-end with tiny params. ~2-5 min cold build."

# Clean start: prevents a stale checkpoint from making the trainer exit early,
# and prevents stale artifacts from masking a broken precompute step.
rm -rf "$SMOKE_DIR"
mkdir -p "$SMOKE_DIR"

# Absolute path so tests that cargo runs from a different CWD can still find it.
SMOKE_DIR_ABS="$(cd "$SMOKE_DIR" && pwd)"

echo "==> [1/4] hand_ranks.bin"
cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    hand_ranks "$SMOKE_DIR_ABS/hand_ranks.bin"

echo "==> [2/4] centroids.bin (k=8, samples=200)"
cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    centroids 200 8 "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/centroids.bin"

echo "==> [3/4] preflop_abstraction.bin"
EHS_SAMPLES=20 cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- \
    preflop "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" \
    "$SMOKE_DIR_ABS/preflop_abstraction.bin"

echo "==> [4/4] pkr-trainer (10 iterations, capacity=4096)"
EHS_SAMPLES=20 cargo run --release -p pkr-trainer -- \
    --iterations 10 \
    --threads 2 \
    --capacity 4096 \
    --centroids "$SMOKE_DIR_ABS/centroids.bin" \
    --preflop-table "$SMOKE_DIR_ABS/preflop_abstraction.bin" \
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

echo "==> Loading the produced blueprint through pkr-runtime"
PKR_BLUEPRINT="$SMOKE_DIR_ABS/blueprint.bin" \
    cargo test --release -p pkr-trainer --test pipeline -- --ignored load_external_blueprint

echo "=== smoke test passed ==="
