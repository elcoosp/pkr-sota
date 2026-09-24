#!/usr/bin/env bash
# Runs every criterion bench and emits Bencher-shaped JSON to $BENCH_OUT
# (default bench-results.ndjson). Also runs the thread-scaling bench and
# parses the BENCH lines into a JSON summary.
#
# NOTE (worklog B10): the plan draft contained two consecutive loops over
# BENCH_PKG, the first referencing an undefined $BENCH_PKG_NAME. Only the
# second (run-all-benches-in-pkg) loop is kept here.
set -euo pipefail
cd "$(dirname "$0")/../.."

BENCH_OUT="${BENCH_OUT:-bench-results.ndjson}"
SMOKE_DIR="${SMOKE_DIR:-./outputs/v0-smoke}"

# 1) Pre-reqs — smoke artifacts must exist (or be restored from cache).
if [ ! -s "$SMOKE_DIR/hand_ranks.bin" ]; then
    echo "ERROR: $SMOKE_DIR/hand_ranks.bin missing. Run smoke first."
    exit 1
fi

export PKR_HAND_RANKS="$SMOKE_DIR/hand_ranks.bin"
export PKR_CENTROIDS="$SMOKE_DIR/centroids.bin"
export PKR_BLUEPRINT="$SMOKE_DIR/blueprint.bin"
export PKR_PREFLOP_TABLE="$SMOKE_DIR/preflop_abstraction.bin"
export PKR_FLOP_TABLE="$SMOKE_DIR/flop_abstraction.bin"
export PKR_TURN_TABLE="$SMOKE_DIR/turn_abstraction.bin"
export PKR_RIVER_TABLE="$SMOKE_DIR/river_buckets.bin"

: > "$BENCH_OUT"

# 2) Run each criterion bench package (all benches in the pkg).
#    --save-baseline records to target/criterion for cross-run comparison.
for BENCH_PKG in pkr-contracts-bench pkr-core-bench pkr-eval-bench \
                 pkr-runtime-bench pkr-cfr-bench pkr-abstraction-bench; do
    echo "==> criterion (all benches in pkg): $BENCH_PKG"
    cargo bench -p "$BENCH_PKG" -- \
        --save-baseline ci-nightly --output-format=bencher \
        >> "$BENCH_OUT" 2>/tmp/criterion-$BENCH_PKG.log || true
    tail -2 /tmp/criterion-$BENCH_PKG.log || true
done

# 3) Thread-scaling bench via bench.sh.
echo "==> thread scaling"
THREADS_LIST="1 2 4 8" SECONDS_PER_RUN=10 \
    ./bench.sh > /tmp/bench-scaling.log 2>&1 || true
python3 ci/scripts/parse-bench-scaling.py /tmp/bench-scaling.log \
    >> "$BENCH_OUT"

echo "==> bench results in $BENCH_OUT"
