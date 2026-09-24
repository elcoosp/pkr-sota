#!/usr/bin/env python3
"""Parse stats.json and emit Bencher-shaped JSON lines for
cumulative metrics that aren't in metrics.csv's last row.

Key names follow the real `serde_json::json!` block in
binaries/pkr-trainer/src/main.rs (verified: strategy_analysis uses
`mean_entropy`, `pure`, `mixed`, `empty` — not the draft names).
"""
import datetime
import json
import sys

if len(sys.argv) < 2:
    print("usage: parse-stats-json.py <stats.json>", file=sys.stderr)
    sys.exit(1)

with open(sys.argv[1]) as f:
    s = json.load(f)

now = datetime.datetime.now(datetime.timezone.utc).isoformat()


def emit(name, value, unit, hib=None):
    if value is None:
        return
    try:
        v = float(value)
    except (TypeError, ValueError):
        return
    print(json.dumps({
        "benchmark": f"stats/{name}",
        "value": v,
        "unit": unit,
        "higher_is_better": hib,
        "timestamp": now,
    }))


def pick(d, *keys):
    for k in keys:
        if isinstance(d, dict) and k in d and d[k] is not None:
            return d[k]
    return None


snap = s.get("snapshot", {})
cum = s.get("cumulative_metrics", {})
sa = s.get("strategy_analysis", {})

emit("wall_seconds",          s.get("wall_seconds"),          "s",     False)
emit("infosets",              snap.get("infosets"),           "count", True)
emit("capacity_pct",          snap.get("capacity_pct"),       "pct",   False)
emit("max_abs_regret",        snap.get("max_abs_regret"),     "abs",   False)
emit("mean_abs_regret",       snap.get("mean_abs_regret"),    "abs",   False)
emit("nonfinite_count",       snap.get("nonfinite_count"),    "count", False)
emit("strategy_sum_mass",     snap.get("strategy_sum_mass"),  "mass",  None)
emit("nodes_per_iteration",   cum.get("nodes_per_iteration"), "count", None)
emit("avg_depth",             cum.get("avg_depth"),           "depth", None)
emit("max_depth",             cum.get("max_depth"),           "depth", None)
emit("cache_hit_rate",        cum.get("cache_hit_rate"),      "ratio", True)
emit("infosets_created",      cum.get("infosets_created"),    "count", True)
emit("regret_dedup_ratio",    cum.get("regret_dedup_ratio"),  "ratio", True)
emit("total_traverse_s",      cum.get("total_traverse_s"),    "s",     False)
emit("total_merge_s",         cum.get("total_merge_s"),       "s",     False)
emit("total_flush_s",         cum.get("total_flush_s"),       "s",     False)
emit("mean_entropy_bits",     pick(sa, "mean_entropy", "mean_entropy_bits"), "bits", None)
emit("strategy_pure",         pick(sa, "pure", "strategy_pure"),   "count", None)
emit("strategy_mixed",        pick(sa, "mixed", "strategy_mixed"), "count", None)
emit("strategy_empty",        pick(sa, "empty", "strategy_empty"), "count", False)
