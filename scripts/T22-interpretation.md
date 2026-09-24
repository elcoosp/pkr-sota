# T2.2 interpretation guide

Once `outputs/v26a/exploitability.csv` has enough rows, use this to
decide whether T2.2 was worth keeping.

## The comparison

Reference: v25final (pre-T2.2), 200M iters, 8 threads.


## Precompute drift caveat

The river table for v26a was generated with `EHS_SAMPLES=30` and
`RIVER_OUTER_SAMPLES=50` (vs the old 100/200 defaults) to make regen
~13x faster. A 2,000-board simulation showed **98.15% pairwise
same-bucket agreement** between the two regimes.

This means: for a given board, ~1.85% of the time it lands in a
different k-means bucket than it would have at full quality.

### How this affects the interpretation

- If v26a shows a **big win** (>2σ below v25final at 100M+),
  the drift is irrelevant — no amount of low-sample noise could
  create a spurious improvement.
- If v26a is **within noise** of v25final, the drift COULD be
  hiding a small T2.2 effect. To rule this out:
    1. Regenerate river with `EHS_SAMPLES=100 RIVER_OUTER_SAMPLES=200`.
    2. Rerun 100M iters with the same config.
    3. Compare.
- If v26a is **worse**, the drift is not the cause (low-sample
  precompute should be if anything noisier, not systematically worse).
