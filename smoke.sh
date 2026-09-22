#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

SMOKE_DIR="${SMOKE_DIR:-./outputs/v0-smoke}"
[ "${SMOKE_FRESH:-0}" = "1" ] && { echo "wiping $SMOKE_DIR"; rm -rf "$SMOKE_DIR"; }
mkdir -p "$SMOKE_DIR"
SMOKE_DIR_ABS="$(cd "$SMOKE_DIR" && pwd)"

echo "=== smoke test ==="

need() {
    local t="$1"; shift
    [ -s "$t" ] && { echo "  [cached] $(basename "$t")"; return 1; }
    "$@"; return 0
}

cargo build --release -p pkr-trainer -p pkr-abstraction 2>&1 | tail -1
pre() { cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- "$@"; }

if ! need "$SMOKE_DIR_ABS/hand_ranks.bin" pre hand_ranks "$SMOKE_DIR_ABS/hand_ranks.bin"; then :; fi
if ! need "$SMOKE_DIR_ABS/centroids.bin" pre centroids 200 8 "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/centroids.bin"; then :; fi
if ! need "$SMOKE_DIR_ABS/preflop_abstraction.bin" env EHS_SAMPLES=20 pre preflop "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/preflop_abstraction.bin"; then :; fi
if ! need "$SMOKE_DIR_ABS/flop_abstraction.bin" env EHS_SAMPLES=5 pre abs5 "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/flop_abstraction.bin"; then :; fi
if ! need "$SMOKE_DIR_ABS/flop_buckets.bin" pre flop "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/flop_buckets.bin" 8; then :; fi
if ! need "$SMOKE_DIR_ABS/river_buckets.bin" env EHS_SAMPLES=5 pre river "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/river_buckets.bin" 8; then :; fi
if ! need "$SMOKE_DIR_ABS/turn_abstraction.bin" env EHS_SAMPLES=1 pre turn "$SMOKE_DIR_ABS/centroids.bin" "$SMOKE_DIR_ABS/hand_ranks.bin" "$SMOKE_DIR_ABS/turn_abstraction.bin" 100; then :; fi

env EHS_SAMPLES=5 cargo run --release --quiet -p pkr-trainer -- \
    --iterations 10 --threads 2 --capacity 4096 \
    --centroids "$SMOKE_DIR_ABS/centroids.bin" \
    --preflop-table "$SMOKE_DIR_ABS/preflop_abstraction.bin" \
    --flop-table "$SMOKE_DIR_ABS/flop_abstraction.bin" \
    --flop-buckets "$SMOKE_DIR_ABS/flop_buckets.bin" \
    --turn-table "$SMOKE_DIR_ABS/turn_abstraction.bin" \
    --river-table "$SMOKE_DIR_ABS/river_buckets.bin" \
    --rank-table "$SMOKE_DIR_ABS/hand_ranks.bin" \
    --checkpoint "$SMOKE_DIR_ABS/train.ckpt" --checkpoint-every 5 --report-every 5 \
    --output "$SMOKE_DIR_ABS/blueprint.bin" \
    --metrics-csv "$SMOKE_DIR_ABS/metrics.csv" --stats-json "$SMOKE_DIR_ABS/stats.json"

test -s "$SMOKE_DIR_ABS/blueprint.bin" || { echo "FAIL: no blueprint"; exit 1; }
TURN_SIZE=$(wc -c < "$SMOKE_DIR_ABS/turn_abstraction.bin" | tr -d ' ')
[ "$TURN_SIZE" = "305377800" ] || { echo "FAIL: turn size"; exit 1; }

PKR_BLUEPRINT="$SMOKE_DIR_ABS/blueprint.bin" \
    cargo test --release --quiet -p pkr-trainer --test pipeline -- --ignored load_external_blueprint

echo "=== smoke passed ==="
