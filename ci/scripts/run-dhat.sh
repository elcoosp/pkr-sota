#!/usr/bin/env bash
# Run a short (5K-iter) proftest with dhat profiling enabled.
# Parse the resulting JSON for total bytes + top alloc sites.
set -euo pipefail
cd "$(dirname "$0")/../.."

PROF_DIR="${PROF_DIR:-./outputs/v0-dhat}"
mkdir -p "$PROF_DIR"

cargo build --release -p pkr-trainer --features dhat-profiling
# Note: dhat disables LTO by overriding the profile; we accept this
# because the goal is to measure memory, not throughput.

PROF_DIR="$PROF_DIR" \
ITERATIONS=5000 THREADS=2 CAPACITY=1000000 \
    ./proftest.sh

python3 - "$PROF_DIR/dhat-out/dhat-heap.json" <<'PY'
import datetime, json, sys
with open(sys.argv[1]) as f:
    d = json.load(f)
now = datetime.datetime.now(datetime.timezone.utc).isoformat()
total = d.get("total_bytes", 0)
peak  = d.get("peak_bytes", 0)
print(json.dumps({
    "benchmark": "memory/total_bytes",
    "value": total,
    "unit": "bytes",
    "higher_is_better": False,
    "timestamp": now,
}))
print(json.dumps({
    "benchmark": "memory/peak_bytes",
    "value": peak,
    "unit": "bytes",
    "higher_is_better": False,
    "timestamp": now,
}))
# Top 5 allocation sites by total bytes
items = sorted(d.get("items", []), key=lambda x: -x.get("total_bytes", 0))[:5]
for it in items:
    print(json.dumps({
        "benchmark": f"memory/site_{it['frame'][:60]}",
        "value": it.get("total_bytes", 0),
        "unit": "bytes",
        "higher_is_better": False,
        "timestamp": now,
    }))
PY
