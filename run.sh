# 1. Generate the hand rank lookup table (~5 seconds)
cargo run --release --bin pkr-abstraction-precompute -- table hand_ranks.bin

# 2. Generate centroids (flop only; repeat with per-street boards for turn/river)
cargo run --release --bin pkr-abstraction-precompute -- centroids 10000 200 centroids.bin

# 3. Generate precomputed abstraction tables
EHS_SAMPLES=1000 cargo run --release --bin pkr-abstraction-precompute -- preflop_table  centroids.bin hand_ranks.bin preflop_abstraction.bin
EHS_SAMPLES=1000 cargo run --release --bin pkr-abstraction-precompute -- abstraction    centroids.bin hand_ranks.bin abstraction.bin
EHS_SAMPLES=1000 cargo run --release --bin pkr-abstraction-precompute -- turn_table     centroids.bin hand_ranks.bin turn_abstraction.bin

# 4. Train (uses global Rayon pool, TableEvaluator, no per-iteration overhead)
cargo run --release --bin pkr-trainer -- \
  --iterations 100000 \
  --threads 8 \
  --centroids centroids.bin \
  --preflop-table preflop_abstraction.bin \
  --flop-table abstraction.bin \
  --turn-table turn_abstraction.bin \
  --rank-table hand_ranks.bin \
  --output blueprint.bin
