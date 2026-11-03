#!/usr/bin/env python3
"""Golden test for ci/scripts/extract-criterion-diff.py (2026-11-03 re-audit M4).

Feeds a real-shaped Criterion 0.5 `--baseline` transcript (time: /
change: / verdict lines, outliers noise, a no-baseline bench) through
parse_stream and asserts the emitted Markdown table byte-for-byte.
Run: python3 ci/scripts/test_extract_criterion_diff.py
"""
import importlib.util
import io
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

SAMPLE = """\
regression/fib10        time:   [1.1143 µs 1.1185 µs 1.1232 µs]
                        change: [-12.345% -11.000% -10.500%] (p = 0.00 < 0.05)
                        Performance has improved.
Found 3 outliers among 100 measurements (3.00%)
  1 (1.00%) low severe
regression/fib20        time:   [21.452 µs 21.500 µs 21.552 µs]
                        change: [+5.1234% +5.9876% +6.8765%] (p = 0.01 < 0.05)
                        Performance has regressed.
regression/hash_u64     time:   [1.2034 ns 1.2100 ns 1.2166 ns]
                        change: [-0.5432% +0.1234% +0.7654%] (p = 0.42 > 0.05)
                        No change in performance detected.
regression/noisy        time:   [4.2000 µs 4.2500 µs 4.3000 µs]
                        change: [-1.9000% +0.0000% +1.9000%] (p = 0.61 > 0.05)
                        Change within noise threshold.
brand_new_bench         time:   [9.5000 ns 9.6000 ns 9.7000 ns]
"""

EXPECTED = """\
| benchmark | PR | change | verdict |
|---|---|---|---|
| `regression/fib10` | 1.1185 µs | -11.000% (p = 0.00) | improved |
| `regression/fib20` | 21.500 µs | +5.9876% (p = 0.01) | regressed |
| `regression/hash_u64` | 1.2100 ns | +0.1234% (p = 0.42) | no change |
| `regression/noisy` | 4.2500 µs | +0.0000% (p = 0.61) | noise |
| `brand_new_bench` | 9.6000 ns | n/a | n/a |
"""


def load_module():
    spec = importlib.util.spec_from_file_location(
        "extract_criterion_diff",
        os.path.join(HERE, "extract-criterion-diff.py"),
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)  # noqa: PLW0129 - main() is guarded
    return mod


def main():
    mod = load_module()
    buf = io.StringIO()
    mod.parse_stream(io.StringIO(SAMPLE), buf)
    # parse_stream emits rows only; main() adds the two header lines.
    got = ("| benchmark | PR | change | verdict |\n"
           "|---|---|---|---|\n" + buf.getvalue())
    if got != EXPECTED:
        print("FAIL: parsed table mismatch")
        print("--- expected ---")
        print(EXPECTED)
        print("--- got ---")
        print(got)
        return 1
    # Empty input -> headers only (never crashes, old parser did the same).
    buf2 = io.StringIO()
    mod.parse_stream(io.StringIO(""), buf2)
    if buf2.getvalue() != "":
        print("FAIL: empty input produced output:", repr(buf2.getvalue()))
        return 1
    print("PASS: extract-criterion-diff golden test "
          f"({SAMPLE.count('time:')} benches parsed)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
