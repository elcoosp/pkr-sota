#!/usr/bin/env python3
"""Parse trainer stderr lines of the form:
    EVAL iter=10000 expl_mbb=1234.56+/-12.30 insample=1200.00 br0=0.1200 br1_to_p0=-0.3400 deals=2000
and emit one Bencher line per (iter, metric).

2026-11-03 re-audit (M15): the regex only matched `EVAL iter=...`, so the
trainer's heldout line
    EVAL-HELDOUT iter=5000000 lower_mbb=5341.85+/-406.73 insample=1200.00 fit=0.99 score=0.98
(binaries/pkr-trainer/src/main.rs:1038) produced no Bencher metrics.
Both forms are now parsed; the heldout form emits the lower_mbb metric
(and insample), the plain form keeps expl_mbb/br0/br1_to_p0.

NOTE (worklog B15): the plan draft regex expected
`br1_to_p0=...`. The real trainer (binaries/pkr-trainer/src/main.rs)
prints `br0=... br1=...`:
    EVAL iter={} expl_mbb={:.2}+/-{:.2} insample={:.2} br0={:.4} br1={:.4} deals={}
This parser accepts both spellings (`br1` and `br1_to_p0`). The legacy
heldout spelling `EVAL iter=... heldout_lower_mbb=...` (main.rs:724) is
accepted too.
"""
import datetime
import json
import re
import sys

# Proper float literal (the naive [\d.eE+-]+ class greedily eats the
# '+' of the +/- separator when nothing follows forces a backtrack —
# that bit the heldout patterns, whose line ends after the number).
NUM = r"-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?"

pat = re.compile(
    rf"EVAL iter=(\d+) expl_mbb=({NUM})(?:\+/-{NUM})?"
    rf"(?: insample={NUM})? br0=({NUM}) br1(?:_to_p0)?=({NUM}) deals=(\d+)"
)

# 2026-11-03 re-audit (M15): EVAL-HELDOUT / legacy heldout_lower_mbb.
# The plain pattern above cannot match these (they have no expl_mbb),
# but parse_line tries the heldout shapes first to keep the intent
# explicit.
heldout_pat = re.compile(
    rf"EVAL-HELDOUT iter=(\d+) lower_mbb=({NUM})(?:\+/-{NUM})?"
)
heldout_pat_legacy = re.compile(
    rf"EVAL iter=(\d+) heldout_lower_mbb=({NUM})(?:\+/-{NUM})?"
)

now = datetime.datetime.now(datetime.timezone.utc).isoformat()


def emit(w, it, name, val, unit, hib=False):
    w.write(json.dumps({
        "benchmark": f"nlhe_eval/{name}@{it}",
        "value": val,
        "unit": unit,
        "higher_is_better": hib,
        "timestamp": now,
    }) + "\n")


def parse_line(line, w):
    m = heldout_pat.search(line) or heldout_pat_legacy.search(line)
    if m:
        it = int(m.group(1))
        # Lower bound on the opponent's exploitability: lower is better.
        emit(w, it, "lower_mbb", float(m.group(2)), "mbb")
        return True
    m = pat.search(line)
    if not m:
        return False
    it = int(m.group(1))
    emit(w, it, "expl_mbb", float(m.group(2)), "expl_mbb", hib=False)
    emit(w, it, "br0", float(m.group(3)), "br0", hib=None)
    emit(w, it, "br1_to_p0", float(m.group(4)), "br1_to_p0", hib=None)
    return True


def main():
    with open(sys.argv[1]) as f:
        for line in f:
            parse_line(line, sys.stdout)


if __name__ == "__main__":
    main()
