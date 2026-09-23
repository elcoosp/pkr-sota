#!/usr/bin/env bash
set -euo pipefail

echo "=== Status doc: record what we learned ==="

cat > docs/training-log.md << 'DOCEOF'
# Training log

## 2026-09-23 — eval harness works, k=32 regime is undertrained

Launched `VERSION=v8` at k=32, 15M capacity. Three EVAL checkpoints:

```
EVAL iter=102400 expl_mbb=38186.90
EVAL iter=204800 expl_mbb=38106.94
EVAL iter=307200 expl_mbb=38277.05
```

Interpretation:
- Eval machinery is correct (checked by hand against the formula).
- Curve is flat because CFR is undertrained. 14M infosets / 400K
  iterations = 0.03 visits per infoset. Regret matching cannot
  differentiate at that density.
- The BR values themselves (~80 chips/hand) confirm the trained
  strategy is not merely uniform — it commits to actions that a
  best response punishes harder than random play would.

Conclusion: at k=32 the iteration budget needed for convergence is
~1-10 billion, which is 15-150 hours at current throughput. Not worth
running on this hardware at this abstraction.

Next: retrain at k=8 (100× fewer infosets). Regenerate abstraction
tables once (~45 min turn table). Then 100M iterations (~90 min) gives
~670 visits per infoset. That's the first regime where the eval curve
is expected to decline.
DOCEOF

git add -A
git diff --cached --quiet && echo "nothing" || git commit -m "docs: training log records k=32 undertraining, k=8 next"

echo ""
echo "=== Launch k=8 training in background ==="
echo ""
echo "Config:"
echo "  VERSION=v9"
echo "  k=8 across all streets (preflop, flop, river, turn centroid file)"
echo "  capacity 5M (k=8 produces ~150K infosets total)"
echo "  iterations 100M"
echo "  eval every 5M, 500 deals"
echo ""
echo "Precompute: turn table regenerates once at k=8. ~45 min."
echo "Training: ~90 min at ~20K it/s."
echo "Total: ~2.5 hours. Runs in background."
echo ""

mkdir -p outputs/v9

cat > run-v9.sh << 'RUNEOF'
#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
export RUSTFLAGS="-C target-cpu=native"
export RAYON_NUM_THREADS=8

OUT=outputs/v9
mkdir -p "$OUT"

cargo build --release -p pkr-trainer -p pkr-abstraction 2>&1 | tail -1

pre() {
    local target=""
    for arg in "$@"; do [[ "$arg" == *.bin ]] && target="$arg"; done
    if [ "${REBUILD:-0}" != "1" ] && [ -n "$target" ] && [ -s "$target" ]; then
        echo "  [cached] $1 -> $(basename "$target")"
        return 0
    fi
    cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- "$@"
}

echo "==> k=8 abstraction for v9"
pre hand_ranks "$OUT/hand_ranks.bin"
pre centroids 1000 8 "$OUT/hand_ranks.bin" "$OUT/centroids.bin"
pre flop "$OUT/hand_ranks.bin" "$OUT/flop_buckets.bin" 8
pre river "$OUT/hand_ranks.bin" "$OUT/river_buckets.bin" 8
pre preflop "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/preflop_abstraction.bin"
pre abs5 "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/abstraction.bin"

echo "==> turn table (45 min, one time)"
if [ -s "$OUT/turn_abstraction.bin" ]; then
    echo "  [cached] turn"
else
    EHS_SAMPLES=10 cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute -- \
        turn "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/turn_abstraction.bin" 10000
fi

echo "==> TRAIN 100M iterations at k=8 (90 min)"
cargo run --release --quiet -p pkr-trainer -- \
    --iterations 100000000 \
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
    --checkpoint-every 20000000 \
    --report-every 1000000 \
    --eval-every 5000000 \
    --eval-deals 500 \
    --output "$OUT/blueprint.bin" \
    --metrics-csv "$OUT/metrics.csv" \
    --stats-json "$OUT/stats.json"

echo "=== DONE ==="
RUNEOF
chmod +x run-v9.sh

echo "Launch in tmux (recommended — survives logout):"
echo ""
echo "  tmux new-session -d -s pkr './run-v9.sh 2>&1 | tee /tmp/v9.log'"
echo "  tmux attach -t pkr"
echo ""
echo "Or straight to background:"
echo ""
echo "  nohup ./run-v9.sh > /tmp/v9.log 2>&1 &"
echo "  tail -f /tmp/v9.log"
echo ""
echo "Either way, check progress with:"
echo "  grep '^EVAL' /tmp/v9.log"
echo ""
echo "Expected: first EVAL at 5M iterations (~5 min). Curve should"
echo "decline — 5M/150K = 33 visits/infoset at that point. By 100M"
echo "you're at 670 visits and the curve should show real movement."
echo ""
echo "To stop early once the curve flattens: Ctrl-C the tmux session, or"
echo "  pkill -f pkr-trainer"
