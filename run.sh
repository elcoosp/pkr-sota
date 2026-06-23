#!/usr/bin/env bash
set -euo pipefail
export RUSTFLAGS="-C target-cpu=native"

# ================== CONFIGURABLE PARAMETERS ==================
THREADS=4
ITERATIONS=1000000
CENTROID_SAMPLES=1000
CENTROID_K=200
FLOP_BUCKETS=200
EHS_SAMPLES=100       # <--- 100 is standard for precompute. 1000 is overkill.
# =============================================================

export RAYON_NUM_THREADS=$THREADS
export EHS_SAMPLES=$EHS_SAMPLES

echo "========================================="
echo "  pkr-sota GPU TRAINING PIPELINE"
echo "  Threads       : $THREADS"
echo "  Iterations    : $ITERATIONS"
echo "  EHS samples   : $EHS_SAMPLES"
echo "  Centroids     : $CENTROID_K clusters"
echo "  Flop buckets  : $FLOP_BUCKETS"
echo "========================================="

echo "==> Building release binary with GPU support..."
cargo build --release --features gpu 2>&1 | tail -2

echo "==> [1/6] Generating hand rank table..."
time cargo run --release --features gpu --bin pkr-abstraction-precompute -- table hand_ranks.bin

echo "==> [2/6] Generating centroids..."
time cargo run --release --features gpu --bin pkr-abstraction-precompute -- centroids $CENTROID_SAMPLES $CENTROID_K centroids.bin

echo "==> [3/6] Generating flop buckets..."
time cargo run --release --features gpu --bin pkr-abstraction-precompute -- flop_buckets $FLOP_BUCKETS hand_ranks.bin flop_buckets.bin

echo "==> [4/6] Preflop abstraction table..."
time cargo run --release --features gpu --bin pkr-abstraction-precompute -- preflop_table centroids.bin hand_ranks.bin preflop_abstraction.bin

echo "==> [5/6] Flop abstraction table..."
time cargo run --release --features gpu --bin pkr-abstraction-precompute -- abstraction centroids.bin hand_ranks.bin abstraction.bin

# STEP 6 IS SKIPPED! Depth-Limited CFR evaluates the Turn at runtime using the blueprint,
# so we don't need to precompute a 300M entry Turn table. This saves 11 hours!
# echo "==> [6/6] Turn abstraction table..."
# time cargo run --release --features gpu --bin pkr-abstraction-precompute -- turn_table centroids.bin hand_ranks.bin turn_abstraction.bin

echo "==> [TRAIN] Running $ITERATIONS iterations of PCFR+ (GPU Accelerated)..."
# Note: I removed the --turn-table argument from the trainer command
time cargo run --release --features gpu --bin pkr-trainer -- \
  --iterations $ITERATIONS \
  --threads $THREADS \
  --centroids centroids.bin \
  --preflop-table preflop_abstraction.bin \
  --flop-table abstraction.bin \
  --flop-buckets flop_buckets.bin \
  --rank-table hand_ranks.bin \
  --output blueprint.bin

echo "========================================="
echo "  Training complete. Blueprint: blueprint.bin"
echo "========================================="
