#!/usr/bin/env python3
"""Golden test for ci/scripts/parse-eval-stderr.py (2026-11-03 re-audit M15).

Feeds both trainer stderr line forms — the plain EVAL line (expl_mbb,
br0, br1) and the EVAL-HELDOUT line (lower_mbb) that the old parser
silently dropped — plus the legacy heldout spelling, and asserts the
exact Bencher metric names/values emitted (timestamps normalized).

Run: python3 ci/scripts/test_parse_eval_stderr.py
"""
import importlib.util
import io
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

SAMPLE_LINES = [
    # plain form (binaries/pkr-trainer/src/main.rs:754/1090)
    "EVAL iter=10000 expl_mbb=1234.56+/-12.30 insample=1200.00 "
    "br0=0.1200 br1=-0.3400 deals=2000",
    # heldout form (main.rs:1038) — produced NO metrics before M15
    "EVAL-HELDOUT iter=5000000 lower_mbb=5341.85+/-406.73 "
    "insample=1199.40 fit=0.99 score=0.98",
    # legacy heldout spelling (main.rs:724)
    "EVAL iter=7000000 heldout_lower_mbb=4980.12+/-301.44 insample=1150.10 "
    "fit=0.99 score=0.98",
    # unrelated log noise must be ignored
    "WARN capacity 62.1% infosets=12000000",
]

EXPECTED = [
    ("nlhe_eval/expl_mbb@10000", 1234.56),
    ("nlhe_eval/br0@10000", 0.12),
    ("nlhe_eval/br1_to_p0@10000", -0.34),
    ("nlhe_eval/lower_mbb@5000000", 5341.85),
    ("nlhe_eval/lower_mbb@7000000", 4980.12),
]


def load_module():
    spec = importlib.util.spec_from_file_location(
        "parse_eval_stderr",
        os.path.join(HERE, "parse-eval-stderr.py"),
    )
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def main():
    mod = load_module()
    buf = io.StringIO()
    for line in SAMPLE_LINES:
        mod.parse_line(line, buf)
    got = [json.loads(l) for l in buf.getvalue().splitlines()]
    names = [(g["benchmark"], g["value"]) for g in got]
    if names != EXPECTED:
        print("FAIL: metric stream mismatch")
        for g in got:
            print("  got:", g["benchmark"], g["value"])
        for e in EXPECTED:
            print("  exp:", e[0], e[1])
        return 1
    # invariants: lower_mbb is flagged lower-is-better; other booleans
    # unchanged from the original parser (expl_mbb hib=False).
    by_name = {g["benchmark"]: g for g in got}
    assert by_name["nlhe_eval/lower_mbb@5000000"]["higher_is_better"] is False
    assert by_name["nlhe_eval/expl_mbb@10000"]["higher_is_better"] is False
    assert by_name["nlhe_eval/br1_to_p0@10000"]["higher_is_better"] is None
    print(f"PASS: parse-eval-stderr golden test ({len(got)} metrics: "
          "EVAL expl/br0/br1 + EVAL-HELDOUT lower_mbb + legacy spelling)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
