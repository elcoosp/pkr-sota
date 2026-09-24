#!/usr/bin/env bash
# Reads the last two nightly bench-results.ndjson files from the
# perf-history branch and prints a Markdown table.
# Usage: ./ci/scripts/diff-perf.sh [old.json] [new.json]
set -euo pipefail
OLD="${1:-}"
NEW="${2:-}"
if [ -z "$OLD" ] || [ -z "$NEW" ]; then
    echo "Usage: $0 <old.ndjson> <new.ndjson>"
    exit 1
fi

python3 - "$OLD" "$NEW" <<'PY'
import json, sys
old = {json.loads(l)["benchmark"]: json.loads(l) for l in open(sys.argv[1]) if l.strip()}
new = {json.loads(l)["benchmark"]: json.loads(l) for l in open(sys.argv[2]) if l.strip()}
keys = sorted(set(old) | set(new))
print("| benchmark | old | new | Δ% | note |")
print("|---|---|---|---|---|")
for k in keys:
    o = old.get(k, {}).get("value")
    n = new.get(k, {}).get("value")
    if o is None or n is None:
        print(f"| `{k}` | {o} | {n} | — | missing |")
        continue
    pct = (n - o) / o * 100 if o != 0 else 0.0
    arrow = "🔴" if pct > 5 else ("🟢" if pct < -5 else "→")
    print(f"| `{k}` | {o:.1f} | {n:.1f} | {pct:+.1f}% {arrow} | |")
PY
