#!/usr/bin/env bash
set -euo pipefail
export RUSTFLAGS="-C target-cpu=native"

THREADS=4
ITERATIONS=1000000
CENTROID_SAMPLES=1000
CENTROID_K=200
FLOP_BUCKETS=200
EHS_SAMPLES=100

export RAYON_NUM_THREADS=$THREADS
export EHS_SAMPLES=$EHS_SAMPLES

echo "========================================="
echo "  pkr-sota TRAINING PIPELINE"
echo "  Threads       : $THREADS"
echo "  Iterations    : $ITERATIONS"
echo "  EHS samples   : $EHS_SAMPLES"
echo "  Centroids     : $CENTROID_K clusters"
echo "  Flop buckets  : $FLOP_BUCKETS"
echo "========================================="

echo "==> Building release binaries..."
cargo build --release -p pkr-trainer -p pkr-abstraction

echo "==> [1/6] Generating hand rank table..."
time cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- hand_ranks hand_ranks.bin

echo "==> [2/6] Generating centroids..."
time cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- centroids $CENTROID_SAMPLES $CENTROID_K hand_ranks.bin centroids.bin

echo "==> [3/6] Generating flop buckets..."
time cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- flop hand_ranks.bin flop_buckets.bin $FLOP_BUCKETS

echo "==> [4/6] Preflop abstraction table..."
time cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- preflop centroids.bin hand_ranks.bin preflop_abstraction.bin

echo "==> [5/6] Flop abstraction table..."
time cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- abs5 centroids.bin hand_ranks.bin abstraction.bin

echo "==> [TRAIN] Running $ITERATIONS iterations..."
time cargo run --release -p pkr-trainer -- \
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
