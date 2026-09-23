#!/usr/bin/env bash
# run-v12.sh - T0.2-clean training run.
#
# Same structure as v11 but:
#   - fresh output dir outputs/v12/
#   - abstraction tables symlinked from outputs/v9/ (unchanged by T0.2)
#   - fresh checkpoint (never reuses v9 ckpts; those carry pre-T0.2 semantics)
#   - refuses to start on a dirty tree unless ALLOW_DIRTY=1
#
# Foreground:  ./run-v12.sh
# Background:  nohup ./run-v12.sh > /tmp/v12_console.log 2>&1 &
#              tail -f /tmp/v12_console.log
# Watch eval:  tail -f /tmp/v12_evals.log

set -uo pipefail
cd "$(dirname "$0")"
export RUSTFLAGS="-C target-cpu=native"

OUT=outputs/v12
SRC=outputs/v9
LOG=/tmp/v12.log
EVAL_LOG=/tmp/v12_evals.log

CHUNK=5000000
TOTAL=40000000
EVAL_HANDS=500

{
    echo "=== pkr-sota v12: T0.2-clean training ==="
    echo "  started:    $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "  output:     $OUT"
    echo "  source:     $SRC (abstraction, symlinked)"
    echo "  chunk size: $CHUNK"
    echo "  total:      $TOTAL"
    echo "  eval:       $EVAL_HANDS hands vs each scripted bot per chunk"
    echo "  code:       $(git rev-parse --short HEAD) on $(git branch --show-current)"
    echo "  dirty:      $(git status --porcelain | wc -l | tr -d ' ') modified files"
    echo ""
} | tee "$LOG"

if [ "${ALLOW_DIRTY:-0}" != "1" ] && [ -n "$(git status --porcelain)" ]; then
    {
        echo "ABORT: working tree is dirty."
        echo ""
        echo "       git status --short:"
        git status --short
        echo ""
        echo "       Commit or stash, then rerun."
        echo "       Override with:  ALLOW_DIRTY=1 ./run-v12.sh"
    } | tee -a "$LOG"
    exit 1
fi

mkdir -p "$OUT"
for f in centroids.bin preflop_abstraction.bin abstraction.bin \
         turn_abstraction.bin river_buckets.bin flop_buckets.bin hand_ranks.bin; do
    if [ ! -e "$SRC/$f" ]; then
        echo "ABORT: missing $SRC/$f" | tee -a "$LOG"
        exit 1
    fi
    ln -sf "../$SRC/$f" "$OUT/$f"
done

if [ -e "$OUT/train.ckpt" ]; then
    echo "  removing stale $OUT/train.ckpt (pre-run cleanup)" | tee -a "$LOG"
    rm -f "$OUT/train.ckpt" "$OUT/train.ckpt.prev"
fi

cat > "$OUT/manifest.txt" << MANEOF
version=v12
source_abstraction=$SRC
chunk=$CHUNK
total=$TOTAL
eval_hands=$EVAL_HANDS
started_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)
git_commit=$(git rev-parse --short HEAD)
git_branch=$(git branch --show-current)
MANEOF

echo "=== building binaries ===" | tee -a "$LOG"
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
        --iters-per-sync 512 \
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
echo "=== v12 COMPLETE at $(date -u +%Y-%m-%dT%H:%M:%SZ) ===" | tee -a "$LOG"
