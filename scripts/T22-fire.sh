#!/usr/bin/env bash
# Fire T2.2: rebuild, regen tables at RIVER_BUCKETS=128, launch v26a.
# Reviewed and tested; run manually when cores are free.
set -euo pipefail
cd "$(dirname "$0")/.."

echo '==> [1/4] release build'
cargo build --release -p pkr-trainer

echo '==> [2/4] prep outputs/v26a'
mkdir -p outputs/v26a
for f in hand_ranks.bin centroids.bin flop_buckets.bin preflop_abstraction.bin abstraction.bin; do
    [ -f outputs/v24/$f ] && cp outputs/v24/$f outputs/v26a/$f
done
rm -f outputs/v26a/river_buckets.bin outputs/v26a/turn_abstraction.bin

echo '==> [3/4] regen tables at RIVER_BUCKETS=128'
# run.sh regenerates only missing artifacts (river + turn).
# Uses PKR_EVALUATOR=fast7 for the fast path.
RIVER_BUCKETS=128 REBUILD=0 SKIP_TRAIN=1 VERSION=v26a PKR_EVALUATOR=fast7 ./run.sh

echo '==> [4/4] launch v26a (200M iters, evals every 20M)'
RIVER_BUCKETS=128 ITERATIONS=200000000 \
    EVAL_EVERY=20000000 EVAL_DEALS=2000 CHECKPOINT_EVERY=20000000 \
    PKR_MOMENTUM=0 PKR_AVG_POWER=2 PKR_EXPLORE_EPSILON=0.01 \
    VERSION=v26a ./run.sh
