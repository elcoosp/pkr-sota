#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
export RUSTFLAGS="-C target-cpu=native"

OUT=outputs/v9
LOG=/tmp/v11.log
EVAL_LOG=/tmp/v11_evals.log

CHUNK=5000000
TOTAL=40000000
EVAL_HANDS=500

{
    echo "=== pkr-sota v11: fresh k=8 training with per-chunk bb/100 eval ==="
    echo "  started: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "  chunk size: $CHUNK"
    echo "  total target: $TOTAL"
    echo "  eval: $EVAL_HANDS hands vs each scripted bot after each chunk"
    echo ""
} | tee "$LOG"

echo "=== building binaries (waiting for lock) ===" | tee -a "$LOG"
cargo build --release -p pkr-trainer -p pkr-abstraction 2>&1 | tee -a "$LOG" | tail -2
BUILD_RC=${PIPESTATUS[0]}
if [ "$BUILD_RC" != "0" ]; then
    echo "  build failed, rc=$BUILD_RC" | tee -a "$LOG"
    exit 1
fi

for ((target = CHUNK; target <= TOTAL; target += CHUNK)); do
    echo "" | tee -a "$LOG"
    echo "=== CHUNK: train to iteration $target at $(date -u +%H:%M:%SZ) ===" | tee -a "$LOG"

    cargo run --release --quiet -p pkr-trainer -- \
        --iterations "$target" \
        --threads 8 \
        --capacity 5000000 \
        --centroids "$OUT/centroids.bin" \
        --preflop-table "$OUT/preflop_abstraction.bin" \
        --flop-table "$OUT/abstraction.bin" \
        --flop-buckets "$OUT/flop_buckets.bin" \
        --turn-table "$OUT/turn_abstraction.bin" \
        --river-table "$OUT/river_buckets.bin" \
        --rank-table "$OUT/hand_ranks.bin" \
        --checkpoint "$OUT/train.ckpt" \
        --checkpoint-every "$CHUNK" \
        --report-every 1000000 \
        --eval-every 0 \
        --output "$OUT/blueprint_${target}.bin" \
        2>&1 | tee -a "$LOG" | tail -3

    RUN_RC=${PIPESTATUS[0]}
    if [ "$RUN_RC" != "0" ]; then
        echo "  chunk run failed, rc=$RUN_RC" | tee -a "$LOG"
        continue
    fi

    if [ ! -f "$OUT/blueprint_${target}.bin" ]; then
        echo "  no blueprint produced for target $target" | tee -a "$LOG"
        continue
    fi

    echo "" | tee -a "$LOG"
    echo "=== EVAL after $target ===" | tee -a "$LOG"

    PKR_BLUEPRINT="$OUT/blueprint_${target}.bin" \
    PKR_CENTROIDS="$OUT/centroids.bin" \
    PKR_PREFLOP_TABLE="$OUT/preflop_abstraction.bin" \
    PKR_FLOP_TABLE="$OUT/abstraction.bin" \
    PKR_TURN_TABLE="$OUT/turn_abstraction.bin" \
    PKR_RIVER_TABLE="$OUT/river_buckets.bin" \
    PKR_FLOP_BUCKETS="$OUT/flop_buckets.bin" \
    PKR_RANK_TABLE="$OUT/hand_ranks.bin" \
    PKR_EVAL_HANDS="$EVAL_HANDS" \
    cargo test --release -q -p pkr-trainer --test eval_harness -- --ignored --nocapture 2>&1 \
        | grep -E "vs |mean |decisions" | tee -a "$EVAL_LOG"

    echo "--- (end of chunk $target) ---" | tee -a "$EVAL_LOG"
done

echo "" | tee -a "$LOG"
echo "=== v11 COMPLETE at $(date -u +%H:%M:%SZ) ===" | tee -a "$LOG"
