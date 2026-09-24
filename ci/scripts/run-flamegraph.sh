#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

cargo install flamegraph 2>&1 | tail -1 || true

# 10K iters is enough sample; ~5 min on a 4-vCPU runner.
PROF_DIR="${PROF_DIR:-./outputs/v0-flame}"
export PROF_DIR
ITERATIONS=10000 THREADS=4 CAPACITY=1000000 \
    ./proftest.sh &
PROF_PID=$!

# Sample the trainer process while it runs.
sleep 5
PID=$(pgrep -f "target/release/pkr-trainer" | head -1 || true)
[ -n "${PID:-}" ] || { echo "FAIL: couldn't find trainer PID"; wait "$PROF_PID"; exit 1; }

sudo flamegraph -o "$PROF_DIR/flame.svg" -p "$PID" -- 15 || true
wait "$PROF_PID"
