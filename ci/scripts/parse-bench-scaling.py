#!/usr/bin/env python3
"""Parse bench.sh stdout and emit one Bencher-shaped JSON line per
threads-config. bench.sh prints `BENCH threads=1 it_per_s=15635`
lines (emitted by the B10 bench.sh edit; the trainer itself only
prints `iter ... | ... it/s` progress lines).

Every number emitted carries a unit and the run timestamp is recorded
by the CI artifact; see ci/bencher.yml.
"""
import datetime
import json
import re
import sys

pat = re.compile(r"BENCH threads=(\d+) it_per_s=([\d.]+)")
now = datetime.datetime.now(datetime.timezone.utc).isoformat()
with open(sys.argv[1]) as f:
    for line in f:
        m = pat.search(line)
        if not m:
            continue
        threads, it_per_s = int(m.group(1)), float(m.group(2))
        print(json.dumps({
            "benchmark": f"trainer/threads_{threads}",
            "value": it_per_s,
            "unit": "it_per_s",
            "higher_is_better": True,
            "timestamp": now,
        }))
