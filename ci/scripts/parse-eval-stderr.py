#!/usr/bin/env python3
"""Parse trainer stderr lines of the form:
    EVAL iter=10000 expl_mbb=1234.56+/-12.30 insample=1200.00 br0=0.1200 br1_to_p0=-0.3400 deals=2000
and emit one Bencher line per (iter, metric).

NOTE (worklog B15): the plan draft regex expected
`br1_to_p0=...`. The real trainer (binaries/pkr-trainer/src/main.rs)
prints `br0=... br1=...`:
    EVAL iter={} expl_mbb={:.2}+/-{:.2} insample={:.2} br0={:.4} br1={:.4} deals={}
This parser accepts both spellings (`br1` and `br1_to_p0`).
"""
import datetime
import json
import re
import sys

pat = re.compile(
    r"EVAL iter=(\d+) expl_mbb=([\d.eE+-]+)(?:\+/-[\d.eE+-]+)?"
    r"(?: insample=[\d.eE+-]+)? br0=([\d.eE+-]+) br1(?:_to_p0)?=([\d.eE+-]+) deals=(\d+)"
)

now = datetime.datetime.now(datetime.timezone.utc).isoformat()
with open(sys.argv[1]) as f:
    for line in f:
        m = pat.search(line)
        if not m:
            continue
        it = int(m.group(1))
        for name, val, hib in [
            ("expl_mbb", float(m.group(2)), False),
            ("br0",      float(m.group(3)), None),
            ("br1_to_p0", float(m.group(4)), None),
        ]:
            print(json.dumps({
                "benchmark": f"nlhe_eval/{name}@{it}",
                "value": val,
                "unit": name,
                "higher_is_better": hib,
                "timestamp": now,
            }))
