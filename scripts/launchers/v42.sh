#!/usr/bin/env bash
# v42 launcher — the first clean post-audit training run.
#
# Includes two preflight checks that v41 lacked:
#
#   1. Binary freshness: refuse to launch if any source file under
#      crates/pkr-cfr/src, crates/pkr-core/src, or binaries/pkr-trainer
#      is newer than the built binary. This is the check that v41
#      needed — v41 was launched on a binary from 11:00 while F2 and
#      F6 landed at 12:28 and 16:05.
#
#   2. Fingerprint sanity: run a 500-iteration smoke and verify the
#      resulting checkpoint has action_legal_v=1. If the binary
#      predates F6, this fails before any real training begins.
#
# Everything in this script is safe to modify; the two checks above
# should survive any change.
#!/usr/bin/env bash
set -uo pipefail
cd /Users/adm/Documents/Repos/pkr-sota

OUT=outputs/v42-post-audit
LOG=$OUT/launcher.log

{
    echo "=== v42 pipeline ==="
    echo "started: $(date)"
    echo

    # --- PREFLIGHT: binary must be newer than the training sources ---
    echo "--- preflight: binary freshness ---"
    BIN=./target/release/pkr-trainer
    if [ ! -x "$BIN" ]; then
        echo "FATAL: $BIN not found"
        exit 1
    fi
    BIN_MTIME=$(stat -f '%m' "$BIN")
    echo "  binary mtime: $(date -r "$BIN_MTIME")"

    NEWEST_SRC=$(find crates/pkr-cfr/src crates/pkr-core/src binaries/pkr-trainer/src \
        -name '*.rs' -type f -exec stat -f '%m %N' {} \; 2>/dev/null | sort -rn | head -1)
    NEWEST_MTIME=$(echo "$NEWEST_SRC" | awk '{print $1}')
    NEWEST_FILE=$(echo "$NEWEST_SRC" | cut -d' ' -f2-)
    echo "  newest source: $NEWEST_FILE @ $(date -r "$NEWEST_MTIME")"

    if [ "$NEWEST_MTIME" -gt "$BIN_MTIME" ]; then
        echo "FATAL: source files newer than binary. Run:"
        echo "  cargo build --release -p pkr-trainer"
        exit 1
    fi
    echo "  OK: binary is fresh"

    # --- Sanity: run the tiny smoke and verify the fingerprint ---
    echo
    echo "--- sanity: fingerprint must have action_legal_v=1 ---"
    TMP=$(mktemp -d)
    PKR_ALLOW_SMALL_K=1 PKR_ALLOW_EHS_FALLBACK=1 \
        "$BIN" \
        --iterations 500 --seed 1 --threads 2 --eval-every 0 \
        --capacity 5000 \
        --centroids outputs/v0-smoke/centroids.bin \
        --preflop-table outputs/v0-smoke/preflop_abstraction.bin \
        --flop-table outputs/v0-smoke/flop_abstraction.bin \
        --turn-table outputs/v0-smoke/turn_abstraction.bin \
        --river-table outputs/v0-smoke/river_buckets.bin \
        --rank-table outputs/v0-smoke/hand_ranks.bin \
        --output "$TMP/bp.bin" --checkpoint "$TMP/ck.ckpt" \
        --fresh > "$TMP/sanity.log" 2>&1
    ALV=$(python3 -c "
data = open('$TMP/ck.ckpt','rb').read(60)
print(data[12:52][35])
")
    rm -rf "$TMP"
    echo "  action_legal_v = $ALV"
    if [ "$ALV" != "1" ]; then
        echo "FATAL: preflight produced action_legal_v=$ALV, expected 1"
        exit 1
    fi
    echo "  OK: binary writes the F6 fingerprint"

    # --- Launch training ---
    echo
    echo "--- launch trainer ---"
    unset PKR_MOMENTUM PKR_AVG_POWER PKR_EXPLORE_EPSILON PKR_DCFR_ALPHA
    unset PKR_HS_DCFR PKR_HS_DCFR_TOTAL PKR_ALT_UPDATES PKR_PHASE_PROFILE
    unset PKR_RM_PLUS PKR_F5_SEQUENTIAL PKR_STRICT_BETS PKR_SKIP_FORCED
    unset PKR_AVG_AT_TRAVERSER

    nohup "$BIN" \
        --iterations 30000000 \
        --seed 42 \
        --threads 8 \
        --capacity 60000000 \
        --eval-every 3000000 \
        --eval-deals 5000 \
        --promote-gate 3 \
        --promote-min-sigma 2 \
        --stop-on-plateau 5 \
        --checkpoint-every 5000000 \
        --report-every 1000000 \
        --centroids     "$OUT/centroids.bin" \
        --preflop-table "$OUT/preflop_abstraction.bin" \
        --flop-table    "$OUT/abstraction.bin" \
        --flop-buckets  "$OUT/flop_buckets.bin" \
        --turn-table    "$OUT/turn_abstraction.bin" \
        --river-table   "$OUT/river_buckets.bin" \
        --rank-table    "$OUT/hand_ranks.bin" \
        --evaluator table \
        --output             "$OUT/blueprint.bin" \
        --checkpoint         "$OUT/train.ckpt" \
        --metrics-csv        "$OUT/metrics.csv" \
        --stats-json         "$OUT/stats.json" \
        --exploitability-csv "$OUT/exploitability.csv" \
        --fresh \
        > "$OUT/train.log" 2>&1 &
    TPID=$!
    echo "$TPID" > "$OUT/train.pid"
    echo "trainer pid=$TPID"
    echo "pipeline done at $(date)"
} > "$LOG" 2>&1
