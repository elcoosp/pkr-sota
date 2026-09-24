#!/usr/bin/env bash
# T2.2 launch script. READ scripts/T22-river-resolution.md FIRST.
# Template only. Does NOT auto-execute.
set -euo pipefail
cd "$(dirname "$0")/.."

echo '==> Patching lib.rs: river hand_rank >> 15 -> >> 13'
if grep -q 'hand_rank >> 15' crates/pkr-abstraction/src/lib.rs; then
    sed -i.bak 's/let hand_bucket = hand_rank >> 15;.*/let hand_bucket = hand_rank >> 13; \/\/ T2.2/' \
        crates/pkr-abstraction/src/lib.rs
    echo '  patched'
else
    echo '  already patched (or pattern not found)'
fi

echo '==> Regenerating river + turn tables (RIVER_BUCKETS=128)'
RIVER_BUCKETS=128 REBUILD=1 SKIP_TRAIN=1 VERSION=v26a PKR_EVALUATOR=fast7 ./run.sh

echo '==> Launching v26a: 200M iters, T2.2 resolution'
RIVER_BUCKETS=128 ITERATIONS=200000000 \
    EVAL_EVERY=20000000 EVAL_DEALS=2000 CHECKPOINT_EVERY=20000000 \
    PKR_MOMENTUM=0 PKR_AVG_POWER=2 PKR_EXPLORE_EPSILON=0.01 \
    VERSION=v26a ./run.sh
