  wrote patches/T22-river-resolution.patch (git apply --check-able after fix)

=== 2. Fix the stale T2.2 section of the audit ===

=== 3. Result-analysis script for T2.2 / ε A/B ===
#!/usr/bin/env python3
"""Compare two exploitability CSVs side by side.

Usage:
  scripts/compare-runs.py outputs/v25final/exploitability.csv outputs/v26a/exploitability.csv
"""
import csv
import sys
import os

def load(path):
    if not os.path.exists(path):
        return []
    return [(int(r['iter'])/1e6, float(r['expl_mbb']), float(r['expl_stderr_mbb']))
            for r in csv.DictReader(open(path))]

def main():
    if len(sys.argv) < 3:
        print(__doc__)
        sys.exit(1)
    a_path, b_path = sys.argv[1], sys.argv[2]
    a = load(a_path)
    b = load(b_path)
    if not a or not b:
        print('missing csv')
        sys.exit(1)
    print(f'{chr(34)}A: {a_path}{chr(34)}')
    print(f'{chr(34)}B: {b_path}{chr(34)}')
    print()
    print(f"{'iter(M)':>8}  {'A expl':>10} {'A σ':>6}  {'B expl':>10} {'B σ':>6}  {'delta':>8}  {'σ':>6}")
    for it_a, e_a, s_a in a:
        match = [p for p in b if abs(p[0] - it_a) < 0.5]
        if not match:
            continue
        _, e_b, s_b = match[0]
        d = e_b - e_a
        combo = (s_a**2 + s_b**2) ** 0.5
        sigma = d / combo if combo > 0 else 0
        print(f'{it_a:>8.0f}  {e_a:>10.1f} {s_a:>6.1f}  {e_b:>10.1f} {s_b:>6.1f}  {d:>+8.1f}  {sigma:>+6.2f}')

if __name__ == '__main__':
    main()
