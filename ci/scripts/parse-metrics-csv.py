#!/usr/bin/env python3
"""Parse metrics.csv and emit one Bencher-shaped JSON line per
(column, last-row) pair.

Output goes to stdout; consumed by `bencher run` or by the
branch-history fallback.

Column list matches the frozen CSV header in
binaries/pkr-trainer/src/main.rs:
iter,wall_s,it_per_s,infosets,cap_pct,max_abs_regret,mean_abs_regret,
nonfinite,strat_mass,nodes,nodes_per_iter,avg_depth,max_depth,
cache_hit_rate,regret_in,regret_out,regret_dedup,strategy_applied,
traverse_ms,merge_ms,flush_ms,wall_ms
"""
import csv
import datetime
import json
import sys

if len(sys.argv) < 2:
    print("usage: parse-metrics-csv.py <metrics.csv>", file=sys.stderr)
    sys.exit(1)

path = sys.argv[1]
with open(path) as f:
    rows = list(csv.DictReader(f))
if not rows:
    sys.exit(0)

last = rows[-1]
now = datetime.datetime.now(datetime.timezone.utc).isoformat()

# Columns to trend: (column, unit, higher_is_better)
COLUMNS = [
    ("it_per_s",        "it_per_s", True),
    ("infosets",        "count",    True),
    ("cap_pct",         "pct",      False),
    ("max_abs_regret",  "abs",      False),
    ("mean_abs_regret", "abs",      False),
    ("nonfinite",       "count",    False),
    ("strat_mass",      "mass",     None),
    ("nodes_per_iter",  "count",    None),
    ("avg_depth",       "depth",    None),
    ("max_depth",       "depth",    None),
    ("cache_hit_rate",  "ratio",    True),
    ("regret_in",       "count",    None),
    ("regret_out",      "count",    None),
    ("regret_dedup",    "ratio",    True),
    ("strategy_applied","count",    None),
    ("traverse_ms",     "ms",       False),
    ("merge_ms",        "ms",       False),
    ("flush_ms",        "ms",       False),
    ("wall_ms",         "ms",       False),
]

for col, unit, hib in COLUMNS:
    if col not in last or not last[col]:
        continue
    try:
        v = float(last[col])
    except ValueError:
        continue
    print(json.dumps({
        "benchmark": f"proftest/{col}",
        "value": v,
        "unit": unit,
        "higher_is_better": hib,
        "timestamp": now,
    }))
