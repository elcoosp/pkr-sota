#!/usr/bin/env python3
"""Parse `cargo bench --baseline X --noplot` output into a Markdown
table. Criterion prints lines like:
    fnv1a/u64_input      12.3 ns/iter ± 0.5  (± 4.1%)  1.05x slower
"""
import re
import sys

pat = re.compile(
    r"(\S+)\s+([\d.]+\s*\w+/\w+|[\d.]+\s*\w+)\s*±\s*([\d.]+%)\s*"
    r"(?:\(.*?\))?\s*(?:(\d+\.\d+x)\s+(slower|faster))?"
)
print("| benchmark | PR | change | verdict |")
print("|---|---|---|---|")
for line in sys.stdin:
    m = pat.search(line)
    if not m:
        continue
    bench, pr_val, pr_ci, ratio, verdict = m.groups()
    if ratio:
        verdict = f"{ratio} {verdict}"
    else:
        verdict = "noise"
    print(f"| `{bench}` | {pr_val} | {pr_ci} | {verdict} |")
