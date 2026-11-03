#!/usr/bin/env python3
"""Parse `cargo bench --baseline X --noplot` output into a Markdown table.

2026-11-03 re-audit (M4): the workspace uses Criterion 0.5, whose
--baseline output looks like:

    fnv1a/u64_input        time:   [1.23 ns 1.30 ns 1.40 ns]
                           change: [-12.345% -11.000% -10.500%] (p = 0.01 < 0.05)
                           Performance has improved.

The old parser matched Criterion <=0.3's `12.3 ns/iter ± 0.5 (± 4.1%)`
lines, so the PR perf-diff table was always empty. This parser reads the
0.5 `time:`/`change:`/verdict-line shapes (and still emits the same
Markdown columns: benchmark | PR | change | verdict).
"""
import re
import sys

# Criterion 0.5 pads the benchmark name before `time:   [...]`.
TIME_RE = re.compile(r"^\s*(\S+)\s+time:\s+\[([^\]]*)\]")
# change: [-12.345% -11.000% -10.500%] (p = 0.01 < 0.05)
CHANGE_RE = re.compile(
    r"^\s*change:\s*\[([^\]]*)\]\s*(?:\(p\s*=\s*([\d.eE+-]+)[^)]*\))?"
)
# Verdict lines printed under the change line (criterion 0.5 report.rs).
VERDICT_RE = re.compile(
    r"^\s*(Performance has improved\.|Performance has regressed\.|"
    r"No change in performance detected\.|Change within noise threshold\.)"
)

# The canonical --baseline table has no per-benchmark "slower/faster"
# ratio column; derive the verdict word from criterion's own verdict line.
VERDICT_SHORT = {
    "Performance has improved.": "improved",
    "Performance has regressed.": "regressed",
    "No change in performance detected.": "no change",
    "Change within noise threshold.": "noise",
}


def _median_time(bracket):
    """Middle (point-estimate) value of a criterion time bracket.

    Criterion 0.5 prints `[low med high]` as value/unit pairs, e.g.
    `[1.1143 µs 1.1185 µs 1.1232 µs]`, so tokens alternate value, unit.
    """
    parts = bracket.split()
    vals = parts[0::2]
    units = parts[1::2]
    if len(vals) >= 3:
        return f"{vals[1]} {units[1]}" if len(units) > 1 else vals[1]
    return parts[-1] if parts else "?"


def _median_change(bracket):
    """Middle (point-estimate) value of a change bracket `[-a% -b% -c%]`."""
    parts = bracket.split()
    return parts[1] if len(parts) >= 3 else (parts[-1] if parts else "?")


def parse_stream(inf, out):
    pending = None  # (bench, pr_val) awaiting its change/verdict lines
    verdict = "n/a"  # change: value, set when a change: line follows
    for line in inf:
        m = TIME_RE.search(line)
        if m:
            # Flush a previous benchmark whose verdict never arrived
            # (e.g. a new bench with no baseline: no change: lines).
            if pending:
                bench, pr_val = pending
                out.write(f"| `{bench}` | {pr_val} | {verdict} | n/a |\n")
            pending = (m.group(1), _median_time(m.group(2)))
            verdict = "n/a"
            continue
        if pending is None:
            continue
        m = CHANGE_RE.search(line)
        if m:
            change = _median_change(m.group(1))
            p = m.group(2)
            note = f" (p = {p})" if p else ""
            verdict = change + note
            continue
        m = VERDICT_RE.search(line)
        if m:
            bench, pr_val = pending
            out.write(f"| `{bench}` | {pr_val} | {verdict} | "
                      f"{VERDICT_SHORT[m.group(1)]} |\n")
            pending = None
    if pending:
        bench, pr_val = pending
        out.write(f"| `{bench}` | {pr_val} | n/a | n/a |\n")


def main():
    print("| benchmark | PR | change | verdict |")
    print("|---|---|---|---|")
    parse_stream(sys.stdin, sys.stdout)


if __name__ == "__main__":
    main()
