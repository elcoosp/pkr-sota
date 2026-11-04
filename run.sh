#!/usr/bin/env bash
# run.sh — canonical pkr-sota training launcher.
#
# Sources scripts/run-config.sh for all tunables. Any run-vN.sh should
# also source it rather than duplicating values.
#
# Usage:
#   ./run.sh                          # defaults (VERSION=v16)
#   VERSION=v17 ITERATIONS=80000000 ./run.sh
#   REBUILD=1 ./run.sh                # force precompute regen
#   SKIP_TRAIN=1 ./run.sh             # precompute only
#
# This script is safe to `nohup ... &`; it traps SIGINT/SIGTERM and
# writes a final checkpoint before exiting.
set -euo pipefail
cd "$(dirname "$0")"

# shellcheck source=scripts/run-config.sh
source scripts/run-config.sh

# m50 (audit 2026-10-10): the header claims a trap but there was none.
# Install one so a `nohup ... &` run does not lose the last checkpoint
# on SIGINT/SIGTERM. The training wrapper writes to $LOG, and the trainer
# itself handles SIGINT (see binaries/pkr-trainer), so we only need to
# forward the signal and wait for the child to exit.
#
# 2026-11-03 re-audit (M1): the old trap was a no-op — _CHILD_PID was the
# only assignment (never set), so _forward always exited 130 without
# touching the child. Even with a PID, `cargo run … | tee` means a TERM to
# the pipeline leader does not reach the pkr-trainer grandchild (cargo
# does not forward signals). Fix: the training pipeline below is launched
# with `set -m` so it owns a process group, and _forward TERMs that whole
# group (cargo + pkr-trainer + tee), guarded by a ps pgid check so it can
# never kill our own group. The logic is exercised byte-for-byte by a
# standalone harness (see /tmp sigtrap test in the 2026-11-03 audit).
_CHILD_PID=""
_LOCK_HELD=0
_MYPGID="$(ps -o pgid= -p $$ 2>/dev/null | tr -d '[:space:]' || true)"
_forward() {
    trap - INT TERM   # no re-entry
    if [ -n "$_CHILD_PID" ] && kill -0 "$_CHILD_PID" 2>/dev/null; then
        # TERM the pipeline's own process group (set -m) so the signal
        # reaches cargo AND pkr-trainer AND tee; fall back to the leader
        # alone if the child is somehow in our group.
        _pgid="$(ps -o pgid= -p "$_CHILD_PID" 2>/dev/null | tr -d '[:space:]' || true)"
        if [ -n "$_pgid" ] && [ -n "$_MYPGID" ] && [ "$_pgid" != "$_MYPGID" ]; then
            kill -TERM -- -"$_pgid" 2>/dev/null || true
        else
            kill -TERM "$_CHILD_PID" 2>/dev/null || true
        fi
        # Give the trainer its final-checkpoint window before exiting.
        wait "$_CHILD_PID" 2>/dev/null || true
    fi
    exit 130
}
# 2026-11-03 re-audit (M18): serialize runs per version dir. Two
# concurrent run.sh invocations with the same VERSION interleave their
# rolling checkpoint saves on $OUT/train.ckpt. mkdir is atomic across
# mac/linux; no flock dependency.
_acquire_lock() {
    if ! mkdir "$OUT/.lock" 2>/dev/null; then
        echo "ABORT: $OUT/.lock exists — another run.sh appears active for" \
             "this VERSION. Remove the directory only if that run is dead."
        exit 1
    fi
    _LOCK_HELD=1
}
_cleanup() {
    if [ "$_LOCK_HELD" = "1" ]; then
        rmdir "$OUT/.lock" 2>/dev/null || true
    fi
}
trap _forward INT TERM
trap _cleanup EXIT

LOG="${LOG:-/tmp/${VERSION}.log}"
EVAL_LOG="${EVAL_LOG:-/tmp/${VERSION}_evals.log}"
mkdir -p "$OUT"
_acquire_lock

echo "=== pkr-sota ${VERSION} ==="
echo "  started:     $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "  output:      $OUT"
echo "  centroids:   k=$CENTROID_K (samples=$CENTROID_SAMPLES)"
echo "  buckets:     flop=$FLOP_BUCKETS river=$RIVER_BUCKETS"
echo "  ehs:         preflop/flop=$EHS_SAMPLES turn=$EHS_SAMPLES_TURN"
echo "  iterations:  $ITERATIONS (checkpoint every $CHECKPOINT_EVERY)"
echo "  capacity:    $CAPACITY"
echo "  eval:        every $EVAL_EVERY iters, $EVAL_DEALS deals, gate=$PROMOTE_GATE mbb"
echo "  epsilon:     $PKR_EXPLORE_EPSILON"
echo "  evaluator:   $EVALUATOR"
echo "  code:        $(git rev-parse --short HEAD) on $(git branch --show-current)"
echo ""

if [ -n "$(git status --porcelain)" ]; then
    echo "ABORT: tree dirty:"
    git status --short
    exit 1
fi

# ---------------------------------------------------------------
# 1. Precompute (skip if cached; REBUILD=1 forces)
# ---------------------------------------------------------------
PRE="cargo run --release --quiet -p pkr-abstraction --bin pkr-abstraction-precompute --"

pre() {
    local target=""
    for arg in "$@"; do
        if [[ "$arg" == *.bin ]]; then
            target="$arg"
        fi
    done
    if [ "${REBUILD:-0}" != "1" ] && [ -n "$target" ] && [ -s "$target" ]; then
        echo "  [cached] $(basename "$target")"
        return 0
    fi
    if [[ "$1" == "turn" ]]; then
        PKR_EVALUATOR="$EVALUATOR" EHS_SAMPLES="$EHS_SAMPLES_TURN" \
            $PRE "$@"
    else
        PKR_EVALUATOR="$EVALUATOR" EHS_SAMPLES="$EHS_SAMPLES" \
            $PRE "$@"
    fi
}

echo "==> [1/8] hand_ranks"
pre hand_ranks "$OUT/hand_ranks.bin"
echo "==> [2/8] centroids"
pre centroids "$CENTROID_SAMPLES" "$CENTROID_K" "$OUT/hand_ranks.bin" "$OUT/centroids.bin"
echo "==> [3/8] flop_buckets"
pre flop "$OUT/hand_ranks.bin" "$OUT/flop_buckets.bin" "$FLOP_BUCKETS"
echo "==> [4/8] river_buckets"
pre river "$OUT/hand_ranks.bin" "$OUT/river_buckets.bin" "$RIVER_BUCKETS"
echo "==> [5/8] preflop"
if [ "${PREFLOP_RICH:-1}" = "1" ]; then
    # Rich 6D pipeline: (EHS, EHS^2, rank_high/12, rank_low/12, suited,
    # connector). Same k, same table size, ~425 mbb better exploitability
    # than the 2D baseline in the v33 experiment.
    if [ "${REBUILD:-0}" != "1" ] && [ -s "$OUT/preflop_abstraction.bin" ] \
       && [ -s "$OUT/centroids_6d.bin" ]; then
        echo "  [cached] preflop_abstraction.bin (rich 6D)"
    else
        PKR_RICH_CENTROIDS=1 PKR_EVALUATOR="$EVALUATOR" EHS_SAMPLES="$EHS_SAMPLES" \
            $PRE centroids "$CENTROID_SAMPLES" "$CENTROID_K" \
                "$OUT/hand_ranks.bin" "$OUT/centroids_6d.bin"
        PKR_EVALUATOR="$EVALUATOR" EHS_SAMPLES="$EHS_SAMPLES" \
            $PRE preflop-rich "$OUT/centroids_6d.bin" "$OUT/hand_ranks.bin" \
                "$OUT/preflop_abstraction.bin"
    fi
else
    echo "  (legacy 2D path; set PREFLOP_RICH=1 to use rich 6D)"
    pre preflop "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/preflop_abstraction.bin"
fi
echo "==> [6/8] flop"
pre abs5 "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/abstraction.bin"
echo "==> [7/8] turn"
pre turn "$OUT/centroids.bin" "$OUT/hand_ranks.bin" "$OUT/turn_abstraction.bin" 10000

# ---------------------------------------------------------------
# 2. Audit artifacts before training (F1)
# ---------------------------------------------------------------
echo "==> audit"
if [ -x scripts/verify_artifacts.sh ]; then
    scripts/verify_artifacts.sh "$OUT" || exit 1
fi

if [ "${SKIP_TRAIN:-0}" = "1" ]; then
    echo "SKIP_TRAIN=1: stopping after precompute + audit."
    exit 0
fi

# ---------------------------------------------------------------
# 3. Train
# ---------------------------------------------------------------
echo "==> [8/8] train"
# 2026-11-03 re-audit (M1): run the pipeline in the background with job
# control so it owns a process group (PGID = first pipeline member) and
# _forward can TERM cargo + pkr-trainer + tee together. The explicit rc
# capture preserves the old set -e behaviour of a failing pipeline.
set -m
cargo run --release -p pkr-trainer -- \
    --iterations "$ITERATIONS" \
    --bench-seconds "$BENCH_SECONDS" \
    --threads "$THREADS" \
    --capacity "$CAPACITY" \
    --centroids "$OUT/centroids.bin" \
    --preflop-table "$OUT/preflop_abstraction.bin" \
    --flop-table "$OUT/abstraction.bin" \
    --flop-buckets "$OUT/flop_buckets.bin" \
    --turn-table "$OUT/turn_abstraction.bin" \
    --river-table "$OUT/river_buckets.bin" \
    --rank-table "$OUT/hand_ranks.bin" \
    --evaluator "$EVALUATOR" \
    --checkpoint "$OUT/train.ckpt" \
    --checkpoint-every "$CHECKPOINT_EVERY" \
    --report-every "$REPORT_EVERY" \
    --iters-per-sync "$ITERS_PER_SYNC" \
    --eval-every "$EVAL_EVERY" \
    --eval-deals "$EVAL_DEALS" \
    --stop-on-plateau "$STOP_ON_PLATEAU" \
    --promote-gate "$PROMOTE_GATE" \
    --promote-min-sigma "$PROMOTE_MIN_SIGMA" \
    --exploitability-csv "$OUT/exploitability.csv" \
    --metrics-csv "$OUT/metrics.csv" \
    --stats-json "$OUT/stats.json" \
    --output "$OUT/blueprint.bin" \
    2>&1 | tee "$LOG" &
_CHILD_PID=$!
_rc=0
wait "$_CHILD_PID" || _rc=$?
_CHILD_PID=""
set +m
[ "$_rc" -eq 0 ] || exit "$_rc"

echo ""
echo "=== ${VERSION} complete at $(date -u +%Y-%m-%dT%H:%M:%SZ) ==="
