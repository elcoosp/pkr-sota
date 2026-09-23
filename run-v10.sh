#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
export RUSTFLAGS="-C target-cpu=native"

OUT=outputs/v9
LOG=/tmp/v10.log
EVAL_LOG=/tmp/v10_evals.log

CHUNK=10000000            # iterations per chunk
TOTAL=100000000           # stop at 100M
EVAL_HANDS=500

echo "=== pkr-sota v10: chunked training with bb/100 logging ===" | tee "$LOG"
echo "  starting from checkpoint (v9 at 20M)" | tee -a "$LOG"
echo "  chunk size: $CHUNK" | tee -a "$LOG"
echo "  total target: $TOTAL" | tee -a "$LOG"
echo "  eval: $EVAL_HANDS hands vs each scripted bot, after each chunk" | tee -a "$LOG"
echo "" | tee -a "$LOG"

cargo build --release -p pkr-trainer -p pkr-abstraction 2>&1 | tail -1 | tee -a "$LOG"

# Start the loop at the checkpoint's iteration. The trainer reads
# start_iter from the checkpoint and runs until --iterations (absolute).
START=20000000

for ((target = START + CHUNK; target <= TOTAL; target += CHUNK)); do
    echo "" | tee -a "$LOG"
    echo "=== CHUNK: train to iteration $target ===" | tee -a "$LOG"

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
echo "=== v10 COMPLETE ===" | tee -a "$LOG"
echo "  evals logged to $EVAL_LOG" | tee -a "$LOG"
