#!/usr/bin/env bash
# Compose parse-metrics-csv.py + parse-stats-json.py output, then
# push to Bencher (if BENCHER_API_TOKEN set) or save to disk.
set -euo pipefail
PROF_DIR="${1:-./outputs/v0-proftest-ci}"
OUT="${2:-custom-metrics.ndjson}"

: > "$OUT"
python3 ci/scripts/parse-metrics-csv.py "$PROF_DIR/metrics.csv" >> "$OUT"
python3 ci/scripts/parse-stats-json.py  "$PROF_DIR/stats.json"  >> "$OUT"

if [ -n "${BENCHER_API_TOKEN:-}" ]; then
    ./bencher --token "$BENCHER_API_TOKEN" \
        --project pkr-sota \
        run \
        --adapter json \
        --file "$OUT" \
        --branch main \
        --testbed ci-ubuntu-22.04
fi
