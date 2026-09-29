#!/usr/bin/env python3
"""Compare the v33 retest A/B (2D vs 6D preflop) against the original doc.

Reads outputs/v33retest/seed{42,43}-{A2D,B6D}/exploitability.csv and
emits the same "best-of-run readings" and "pooled" tables that
docs/experiments/v33-rich-preflop-confirmed.md uses, so the two can be
placed side by side.

Usage:
    python3 scripts/analysis/compare-v33-retest.py
"""

import csv
import os
import statistics
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
BASE = os.path.join(ROOT, "outputs", "v33retest")


def best_of_run(path):
    """Return (best_mbb, iter_at_best) or None if the file is missing, empty,
    or malformed (e.g. mid-write by the trainer — first line is the header
    but no data rows have landed yet)."""
    if not os.path.isfile(path):
        return None
    try:
        with open(path) as f:
            rows = list(csv.DictReader(f))
    except (OSError, csv.Error):
        return None
    # Filter to rows with parseable iter+expl_mbb.
    good = []
    for r in rows:
        try:
            good.append((float(r["expl_mbb"]), int(r["iter"])))
        except (KeyError, ValueError, TypeError):
            continue
    if not good:
        return None
    return min(good, key=lambda x: x[0])


def main():
    print("=== v33 retest: 2D (A) vs 6D rich (B) preflop ===")
    print()

    by_seed = {}
    for seed in (42, 43):
        a = best_of_run(os.path.join(BASE, f"seed{seed}-A2D", "exploitability.csv"))
        b = best_of_run(os.path.join(BASE, f"seed{seed}-B6D", "exploitability.csv"))
        by_seed[seed] = (a, b)

    # Per-seed table (matches the doc's "Best-of-run readings").
    # Show partial data: if only A has readings, print A's best-of-run
    # and mark B as pending. This is the common case mid-run.
    print("| seed | A (2D) | B (6D) | delta |")
    print("|---|---|---|---|")
    a_vals, b_vals = [], []
    for seed in (42, 43):
        a, b = by_seed[seed]
        a_str = f"{a[0]:.1f} @ {a[1]/1e6:.1f}M" if a else "(missing)"
        b_str = f"{b[0]:.1f} @ {b[1]/1e6:.1f}M" if b else "(pending)"
        if a and b:
            delta_str = f"{b[0]-a[0]:+.1f}"
            a_vals.append(a[0])
            b_vals.append(b[0])
        elif a:
            delta_str = "—"
            a_vals.append(a[0])
        else:
            delta_str = "—"
        print(f"| {seed} | {a_str} | {b_str} | {delta_str} |")

    # Pooled
    if a_vals and not b_vals:
        print()
        print("| | A (2D) partial | B (6D) |")
        print("|---|---|---|")
        mean_a = statistics.mean(a_vals)
        print(f"| pooled A only | {mean_a:.1f} (n={len(a_vals)}) | pending |")
    if a_vals and b_vals:
        pooled_a = statistics.mean(a_vals)
        pooled_b = statistics.mean(b_vals)
        pooled_se_a = statistics.stdev(a_vals) / len(a_vals) ** 0.5 if len(a_vals) > 1 else 0
        pooled_se_b = statistics.stdev(b_vals) / len(b_vals) ** 0.5 if len(b_vals) > 1 else 0
        # Paired SE: use stdev of the per-seed deltas.
        deltas = [by_seed[s][1][0] - by_seed[s][0][0] for s in (42, 43)
                  if by_seed[s][0] and by_seed[s][1]]
        pooled_se_delta = statistics.stdev(deltas) / len(deltas) ** 0.5 if len(deltas) > 1 else 0
        delta = pooled_b - pooled_a
        z = delta / pooled_se_delta if pooled_se_delta > 0 else float("nan")

        print()
        print("| | A (2D) | B (6D) | delta |")
        print("|---|---|---|---|")
        print(f"| pooled | {pooled_a:.1f} | {pooled_b:.1f} | {delta:+.1f} |")
        print(f"| pooled SE | {pooled_se_a:.1f} | {pooled_se_b:.1f} | {pooled_se_delta:.1f} |")
        print(f"| z | | | {z:+.2f} |")
        print()

        # Comparison to original
        print("=== Comparison to original v33 doc ===")
        print()
        print(f"  original pooled delta: -424.7 mbb (z=-6.65)")
        print(f"  retest   pooled delta: {delta:+.1f} mbb (z={z:+.2f})")
        if abs(z) < 2:
            print("  verdict: effect SHRANK below 2 sigma — original likely inflated by noise")
        elif z < 0 and abs(z) >= 2:
            print("  verdict: effect HOLDS — the +425 mbb preflop win survives determinism")
        else:
            print(f"  verdict: unexpected sign (z={z:+.2f}); inspect individual runs")
    else:
        print()
        print("(not all 4 runs have output yet)")

    # Show raw file list for reference.
    print()
    print("=== Source files ===")
    for seed in (42, 43):
        for tag in ("A2D", "B6D"):
            p = os.path.join(BASE, f"seed{seed}-{tag}", "exploitability.csv")
            exists = "OK " if os.path.isfile(p) else "MISS"
            print(f"  {exists} {p}")


if __name__ == "__main__":
    main()
