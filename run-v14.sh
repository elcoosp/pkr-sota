#!/usr/bin/env bash
# run-v14.sh — k=200 preflop/flop/turn abstraction (was k=8).
# Preflop, flop, turn centroids trained at k=200.
# River and flop_buckets still symlinked from v9 (k=8, unchanged).
set -uo pipefail
cd "$(dirname "$0")"
export RUSTFLAGS="-C target-cpu=native"

REPO_ROOT="$(pwd)"
OUT=outputs/v14
SRC=outputs/v9
LOG=/tmp/v14.log
EVAL_LOG=/tmp/v14_evals.log

CHUNK=5000000
TOTAL=20000000
EVAL_HANDS=2000

{
    echo "=== pkr-sota v14: k=200 preflop/flop/turn abstraction ==="
    echo "  started:  $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "  chunk:    $CHUNK"
    echo "  total:    $TOTAL"
    echo "  eval:     $EVAL_HANDS hands per bot per chunk"
    echo "  code:     $(git rev-parse --short HEAD)"
    echo ""
} | tee "$LOG"

if [ -n "$(git status --porcelain)" ]; then
    { echo "ABORT: tree dirty:"; git status --short; } | tee -a "$LOG"
    exit 1
fi

# Verify the new tables exist
for f in centroids.bin preflop_abstraction.bin abstraction.bin turn_abstraction.bin; do
    if [ ! -e "$OUT/$f" ]; then
        echo "ABORT: missing $OUT/$f — run the precompute fix first" | tee -a "$LOG"
        exit 1
    fi
done

# Symlink unchanged tables from v9
for f in river_buckets.bin flop_buckets.bin hand_ranks.bin; do
    ln -sf "$REPO_ROOT/$SRC/$f" "$OUT/$f"
done

rm -f "$OUT/train.ckpt" "$OUT/train.ckpt.prev"

echo "=== building ===" | tee -a "$LOG"
cargo build --release -p pkr-trainer 2>&1 | tail -2 | tee -a "$LOG"

for ((target = CHUNK; target <= TOTAL; target += CHUNK)); do
    echo "" | tee -a "$LOG"
    echo "=== CHUNK: target=$target at $(date -u +%H:%M:%SZ) ===" | tee -a "$LOG"

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
        --iters-per-sync 512 \
        --output "$OUT/blueprint_${target}.bin" \
        2>&1 | tee -a "$LOG" | tail -3

    if [ ! -f "$OUT/blueprint_${target}.bin" ]; then
        echo "  chunk $target FAILED — aborting" | tee -a "$LOG"
        exit 1
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

    echo "--- end of chunk $target ---" | tee -a "$EVAL_LOG"
done

echo "" | tee -a "$LOG"
echo "=== v14 COMPLETE at $(date -u +%Y-%m-%dT%H:%M:%SZ) ===" | tee -a "$LOG"
