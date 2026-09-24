#!/usr/bin/env python3
"""Parse kuhn_experiment.rs stdout into Bencher-shaped JSON lines.

The expected stdout format (from crates/pkr-testgames/src/bin/kuhn_experiment.rs):
    === Kuhn poker: discount x momentum ===
    Nash value to P0: -1/18 = -0.055556

             iter      vanilla        van-mom          canon      canon-mom
              100   1.234e-01    1.234e-01    1.234e-01    1.234e-01
              300   1.234e-01    ...
              ...

    === Final values at t=3000000 ===
      vanilla       expl=1.234e-03  value=-0.055556  max|reg|=...
      ...

We emit one metric per (config, checkpoint) — `kuhn/<config>@<iter>`.
"""
import datetime
import json
import re
import sys

if len(sys.argv) < 2:
    print("usage: parse-kuhn.py <kuhn-results.txt>", file=sys.stderr)
    sys.exit(1)

# Config labels are read from the header row.
cfg_pat = re.compile(r"^\s*iter\s+(.+?)\s*$")
row_pat = re.compile(r"^\s*(\d+)\s+(.+?)\s*$")
nan_pat = re.compile(r"^\s*NaN\s*$")

now = datetime.datetime.now(datetime.timezone.utc).isoformat()
configs: list = []
with open(sys.argv[1]) as f:
    for line in f:
        m = cfg_pat.match(line)
        if m:
            configs = m.group(1).split()
            continue
        m = row_pat.match(line)
        if not m or not configs:
            continue
        it = int(m.group(1))
        rest = m.group(2).split()
        if len(rest) < len(configs):
            continue
        for i, cfg in enumerate(configs):
            tok = rest[i]
            if nan_pat.match(tok):
                # NaN is treated as a failure (very high exploitability).
                v = 1.0e9
            else:
                try:
                    v = float(tok)
                except ValueError:
                    continue
            print(json.dumps({
                "benchmark": f"kuhn/{cfg}@{it}",
                "value": v,
                "unit": "exploitability",
                "higher_is_better": False,
                "timestamp": now,
            }))
