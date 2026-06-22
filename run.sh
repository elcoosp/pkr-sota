export RUSTFLAGS="-C target-cpu=native"
#!/usr/bin/env bash
set -euo pipefail
export RUSTFLAGS="-C target-cpu=native"
# ================== CONFIGURABLE PARAMETERS ==================
THREADS=4                  # Use only Firestorm (performance) cores
ITERATIONS=8000            # ~10 min total with depth-limited CFR
EHS_SAMPLES=20             # ultra-fast Monte Carlo (acceptable for quick test)
CENTROID_SAMPLES=1000      # fast k‑means seeding
CENTROID_K=50              # coarse hand clusters
FLOP_BUCKETS=50            # coarse board buckets
# =============================================================

export RAYON_NUM_THREADS=$THREADS
export EHS_SAMPLES=$EHS_SAMPLES

echo "========================================="
echo "  pkr-sota 10‑MINUTE TRAINING PIPELINE"
echo "  Threads       : $THREADS"
echo "  Iterations    : $ITERATIONS"
echo "  EHS samples   : $EHS_SAMPLES"
echo "  Centroids     : $CENTROID_K clusters"
echo "  Flop buckets  : $FLOP_BUCKETS"
echo "========================================="

# Build once (not timed)
echo "==> Building release binary (one‑time compilation)..."
cargo build --release 2>&1 | tail -2

# 1. Hand rank lookup table
echo "==> [1/6] Generating hand rank table..."
time cargo run --release --bin pkr-abstraction-precompute -- table hand_ranks.bin

# 2. Centroids
echo "==> [2/6] Generating centroids..."
time cargo run --release --bin pkr-abstraction-precompute -- centroids $CENTROID_SAMPLES $CENTROID_K centroids.bin

# 3. Flop buckets
echo "==> [3/6] Generating flop buckets..."
time cargo run --release --bin pkr-abstraction-precompute -- flop_buckets $FLOP_BUCKETS hand_ranks.bin flop_buckets.bin

# 4. Precomputed abstraction tables
echo "==> [4/6] Preflop abstraction table..."
time cargo run --release --bin pkr-abstraction-precompute -- preflop_table centroids.bin hand_ranks.bin preflop_abstraction.bin

echo "==> [5/6] Flop abstraction table..."
time cargo run --release --bin pkr-abstraction-precompute -- abstraction centroids.bin hand_ranks.bin abstraction.bin

echo "==> [6/6] Turn abstraction table..."
time cargo run --release --bin pkr-abstraction-precompute -- turn_table centroids.bin hand_ranks.bin turn_abstraction.bin

# 5. Train
echo "==> [TRAIN] Running $ITERATIONS iterations of PCFR+..."
time cargo run --release --bin pkr-trainer -- \
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
echo "  Wall‑clock times shown above (excluding build)."
echo "========================================="
