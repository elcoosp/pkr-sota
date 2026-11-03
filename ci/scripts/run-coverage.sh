#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

# 2026-11-03 re-audit (M16): the old flow ran the whole workspace test
# suite twice — `cargo llvm-cov --lcov` and then, inside the python
# heredoc, `cargo llvm-cov --summary` re-executed every test. Run the
# tests ONCE and produce both outputs from the same profdata:
#   1. `cargo llvm-cov --workspace --lcov` runs the suite and writes
#      lcov.info (leaving profdata under target/llvm-cov-target/).
#   2. `cargo llvm-cov report --workspace --summary-only` regenerates the
#      summary from that profdata WITHOUT re-running any tests.
SUMMARY="$(mktemp "${TMPDIR:-/tmp}/pkr-coverage-summary.XXXXXX.txt")"
trap 'rm -f "$SUMMARY"' EXIT

cargo llvm-cov --workspace --lcov --output-path lcov.info
cargo llvm-cov report --workspace --summary-only > "$SUMMARY"

# Emit per-crate % as Bencher metrics.
python3 - "$SUMMARY" <<'PY'
import datetime, json, re, sys

now = datetime.datetime.now(datetime.timezone.utc).isoformat()
for line in open(sys.argv[1]):
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
