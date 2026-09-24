#!/usr/bin/env bash
# CI-friendly proftest. Cuts iteration count to 50K (still gives a
# stable it/s signal) and 4 threads (CI runner constraint).
set -euo pipefail
cd "$(dirname "$0")/../.."

PROF_DIR="${PROF_DIR:-./outputs/v0-proftest-ci}"
ITERATIONS="${ITERATIONS:-50000}"
THREADS="${THREADS:-4}"
CAPACITY="${CAPACITY:-5000000}"

export PROF_DIR ITERATIONS THREADS CAPACITY

# Run the existing proftest.sh — it already does precompute-cache +
# train + JSON validate + artifact listing.
./proftest.sh

# Emit a tiny summary JSON for Bencher custom metrics (B13 parses
# the full metrics.csv too, but this is the one-number headline).
python3 - "$PROF_DIR" <<'PY'
import json, os, sys, csv
prof = sys.argv[1]
# Last row of metrics.csv has the cumulative it/s.
with open(os.path.join(prof, "metrics.csv")) as f:
    rows = list(csv.DictReader(f))
    last = rows[-1] if rows else {}
    it_per_s = float(last.get("it_per_s", 0))
    cache_hit = float(last.get("cache_hit_rate", 0))
    cap_pct = float(last.get("cap_pct", 0))
# stats.json has end-of-run snapshot
with open(os.path.join(prof, "stats.json")) as f:
    stats = json.load(f)
snap = stats.get("snapshot", {})
cum = stats.get("cumulative_metrics", {})
print(json.dumps({
    "iterations": int(last.get("iter", 0)),
    "it_per_s": it_per_s,
    "cache_hit_rate": cache_hit,
    "capacity_pct": cap_pct,
    "infosets": snap.get("infosets", 0),
    "max_abs_regret": snap.get("max_abs_regret", 0),
    "nonfinite_count": snap.get("nonfinite_count", 0),
    "nodes_per_iteration": cum.get("nodes_per_iteration", 0),
    "avg_depth": cum.get("avg_depth", 0),
    "max_depth": cum.get("max_depth", 0),
}, indent=2))
PY
