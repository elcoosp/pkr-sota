#!/usr/bin/env bash
set -euo pipefail

# ================== CONFIGURABLE PARAMETERS ==================
THREADS=8                  # number of CPU threads (M1 has 8 cores)
ITERATIONS=50000           # CFR training iterations (30‑min budget)
EHS_SAMPLES=100            # Monte Carlo samples for EHS (100=fast, 1000=quality)
CENTROID_SAMPLES=5000      # samples for k‑means centroids
CENTROID_K=200             # number of hand‑strength clusters
FLOP_BUCKETS=200           # number of board texture clusters
# =============================================================

export RAYON_NUM_THREADS=$THREADS
export EHS_SAMPLES=$EHS_SAMPLES

echo "========================================="
echo "  pkr-sota training pipeline"
echo "  Threads       : $THREADS"
echo "  Iterations    : $ITERATIONS"
echo "  EHS samples   : $EHS_SAMPLES"
echo "  Centroids     : $CENTROID_K clusters"
echo "  Flop buckets  : $FLOP_BUCKETS"
echo "========================================="

# Build with GPU feature and release optimizations
echo "==> Building release binary (GPU enabled)..."
cargo build --release --features gpu 2>&1 | tail -2

# 1. Hand rank lookup table (5 seconds)
echo "==> [1/6] Generating hand rank table..."
cargo run --release --features gpu --bin pkr-abstraction-precompute -- table hand_ranks.bin

# 2. Centroids (adjust sample count for speed)
echo "==> [2/6] Generating centroids ($CENTROID_SAMPLES samples, $CENTROID_K clusters)..."
cargo run --release --features gpu --bin pkr-abstraction-precompute -- centroids $CENTROID_SAMPLES $CENTROID_K centroids.bin

# 3. Flop buckets (board texture clustering)
echo "==> [3/6] Generating flop buckets ($FLOP_BUCKETS buckets)..."
cargo run --release --features gpu --bin pkr-abstraction-precompute -- flop_buckets $FLOP_BUCKETS hand_ranks.bin flop_buckets.bin

# 4. Precomputed abstraction tables
echo "==> [4/6] Preflop abstraction table..."
cargo run --release --features gpu --bin pkr-abstraction-precompute -- preflop_table centroids.bin hand_ranks.bin preflop_abstraction.bin

echo "==> [5/6] Flop abstraction table..."
cargo run --release --features gpu --bin pkr-abstraction-precompute -- abstraction centroids.bin hand_ranks.bin abstraction.bin

echo "==> [6/6] Turn abstraction table..."
cargo run --release --features gpu --bin pkr-abstraction-precompute -- turn_table centroids.bin hand_ranks.bin turn_abstraction.bin

# 5. Train
echo "==> [TRAIN] Running $ITERATIONS iterations of PCFR+..."
cargo run --release --features gpu --bin pkr-trainer -- \
  --iterations $ITERATIONS \
  --threads $THREADS \
  --centroids centroids.bin \
  --preflop-table preflop_abstraction.bin \
  --flop-table abstraction.bin \
  --turn-table turn_abstraction.bin \
  --flop-buckets flop_buckets.bin \
  --rank-table hand_ranks.bin \
  --output blueprint.bin

echo "========================================="
echo "  Training complete. Blueprint: blueprint.bin"
echo "========================================="
