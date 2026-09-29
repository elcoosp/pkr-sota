#!/usr/bin/env bash
# Golden-hash training check.
#
# Runs a short deterministic training job and asserts the exported
# blueprint and (single-threaded) checkpoint match stored hashes.
#
# What is checked, and why:
#
#   blueprint.bin  — deterministic at any thread count. This is the
#                    artifact hosts load. A hash mismatch here means
#                    the trained strategy itself changed.
#
#   train.ckpt     — deterministic at 1 thread (after commit 0a3c747).
#                    Still differs at 4+ threads because the strategy-
#                    sum accumulator folds per-batch in racy order. See
#                    docs/experiments/training-nondeterminism.md.
#                    We hash it at 1 thread for a stricter regression
#                    signal on the full training state, not just the
#                    quantized export.
#
# Update flow when a change is intentional:
#   GOLDEN_UPDATE=1 bash ci/scripts/golden-training.sh
#   git add ci/golden-training.sha256 && git commit
#
# A mismatch without an intentional change is a regression.
set -euo pipefail
cd "$(dirname "$0")/../.."

ITERATIONS="${ITERATIONS:-100000}"
SEED="${SEED:-42}"
EVAL_DEALS="${EVAL_DEALS:-200}"

GOLDEN_FILE="ci/golden-training.sha256"
SMOKE_DIR="outputs/v0-smoke"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

export PKR_ALLOW_SMALL_K=1
export PKR_ALLOW_EHS_FALLBACK=1

run_one() {
    local threads="$1" out="$2"
    mkdir -p "$out"
    ./target/release/pkr-trainer \
        --iterations "$ITERATIONS" \
        --seed "$SEED" \
        --threads "$threads" \
        --capacity 1000000 \
        --eval-every "$ITERATIONS" \
        --eval-deals "$EVAL_DEALS" \
        --promote-gate 0 \
        --stop-on-plateau 0 \
        --centroids     "$SMOKE_DIR/centroids.bin" \
        --preflop-table "$SMOKE_DIR/preflop_abstraction.bin" \
        --flop-table    "$SMOKE_DIR/flop_abstraction.bin" \
        --turn-table    "$SMOKE_DIR/turn_abstraction.bin" \
        --river-table   "$SMOKE_DIR/river_buckets.bin" \
        --rank-table    "$SMOKE_DIR/hand_ranks.bin" \
        --output             "$out/blueprint.bin" \
        --checkpoint         "$out/train.ckpt" \
        --exploitability-csv "$out/exploitability.csv" \
        --fresh \
        > "$out/train.log" 2>&1
}

echo "=== golden-training: ${ITERATIONS} iters, seed=${SEED} ==="

# Fail early if the smoke abstraction is missing.
for f in centroids.bin preflop_abstraction.bin flop_abstraction.bin \
         turn_abstraction.bin river_buckets.bin hand_ranks.bin; do
    if [ ! -s "$SMOKE_DIR/$f" ]; then
        echo "missing smoke artifact: $SMOKE_DIR/$f" >&2
        echo "run smoke.sh once to generate the smoke abstraction" >&2
        exit 1
    fi
done

cargo build --release -p pkr-trainer

# 4 threads: fast, and blueprint.bin is thread-count-independent.
run_one 4 "$WORK/bp"
BP_HASH=$(shasum -a 256 "$WORK/bp/blueprint.bin" | awk '{print $1}')

# 1 thread: the checkpoint is deterministic only at 1 thread.
run_one 1 "$WORK/ckpt"
CKPT_HASH=$(shasum -a 256 "$WORK/ckpt/train.ckpt" | awk '{print $1}')

# Also hash the report on the 1-thread run — it's built from the same
# state that the checkpoint came from.
EXPL_HASH=$(shasum -a 256 "$WORK/ckpt/exploitability.csv" | awk '{print $1}')

if [ "${GOLDEN_UPDATE:-0}" = "1" ]; then
    cat > "$GOLDEN_FILE" <<GOLDEN
# Auto-generated. Regenerate with:
#   GOLDEN_UPDATE=1 bash ci/scripts/golden-training.sh
# Context: ${ITERATIONS} iterations, seed ${SEED}, ${EVAL_DEALS} eval deals.
${BP_HASH}  blueprint.bin
${CKPT_HASH}  train.ckpt
${EXPL_HASH}  exploitability.csv
GOLDEN
    echo "golden hashes updated:"
    cat "$GOLDEN_FILE"
    exit 0
fi

if [ ! -f "$GOLDEN_FILE" ]; then
    echo "no golden file at $GOLDEN_FILE" >&2
    echo "run with GOLDEN_UPDATE=1 to create one" >&2
    exit 1
fi

fail=0

check() {
    local label="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        printf '  OK    %s\n' "$label"
    else
        printf '  FAIL  %s\n' "$label"
        printf '        expected: %s\n' "$expected"
        printf '        actual:   %s\n' "$actual"
        fail=1
    fi
}

EXPECTED_BP=$(awk '$2=="blueprint.bin" {print $1}' "$GOLDEN_FILE")
EXPECTED_CKPT=$(awk '$2=="train.ckpt" {print $1}' "$GOLDEN_FILE")
EXPECTED_EXPL=$(awk '$2=="exploitability.csv" {print $1}' "$GOLDEN_FILE")

check "blueprint.bin"       "$EXPECTED_BP"   "$BP_HASH"
check "train.ckpt"          "$EXPECTED_CKPT" "$CKPT_HASH"
check "exploitability.csv"  "$EXPECTED_EXPL" "$EXPL_HASH"

if [ "$fail" -eq 0 ]; then
    echo "all hashes match"
    exit 0
fi

cat >&2 <<MSG

If this change is intentional:
  GOLDEN_UPDATE=1 bash ci/scripts/golden-training.sh
  git add $GOLDEN_FILE && git commit

Otherwise this is a regression. Compare the two runs:
  bp:   $WORK/bp/train.log
  ckpt: $WORK/ckpt/train.log
MSG
exit 2
