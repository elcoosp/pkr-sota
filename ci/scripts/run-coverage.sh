#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

cargo llvm-cov --workspace --lcov --output-path lcov.info

# Emit per-crate % as Bencher metrics.
python3 - <<'PY'
import json, re, sys
# Parse `cargo llvm-cov --workspace --summary` for per-crate %.
import subprocess
r = subprocess.run(
    ["cargo", "llvm-cov", "--workspace", "--summary"],
    capture_output=True, text=True, check=True,
)
import datetime
now = datetime.datetime.now(datetime.timezone.utc).isoformat()
for line in r.stdout.splitlines():
    m = re.match(r"\s*(pkr-\S+)\s+.*?\s+(\d+\.\d+)%\s*$", line)
    if not m: continue
    crate, pct = m.group(1), float(m.group(2))
    print(json.dumps({
        "benchmark": f"coverage/{crate}",
        "value": pct,
        "unit": "pct",
        "higher_is_better": True,
        "timestamp": now,
    }))
PY
