# Training nondeterminism (2026-09-28)

**Status:** NEW BUG — invalidates same-seed A/B comparisons

## Discovery

Two runs launched with **identical configuration, identical seed (202),
identical tables, identical eval cadence** diverged by iteration 5M:

| metric @ 5,007,360 | 30M run | 100M run |
|---|---|---|
| expl_mbb | 2776.55 | 2743.17 |
| infosets | 1,005,012 | 1,002,518 |
| br0 | 6.2377 | 6.3105 |
| br1 | 4.8685 | 4.6622 |

The **infoset count differs by 2,494 (0.25%)**. If training were
deterministic, both runs would visit exactly the same states and the
counts would match.

## Impact

- **Every same-seed A/B in the project is invalid** for differences
  smaller than the run-to-run divergence. The divergence at 5M is
  ~33 mbb in the reading and 0.25% in the state space; over a full
  run it could be much larger.
- The v38 30M-vs-100M finding (+350-375 mbb) is *probably* still real
  — the magnitude is 10x the seed-level divergence we see here — but
  it can't be cleanly attributed without fixing this.
- The v33 preflop feature win (+425 mbb) is also 10x larger than the
  divergence, so likely robust. But smaller wins (like the v36
  capacity win at -56 mbb) are questionable.

## Root cause (suspected)

Rayon parallelizes over deals within an iteration. The final regret
merge into the shared table is order-dependent for **float
accumulation**: `(a + b) + c ≠ a + (b + c)` in f32/f64.

If the merge order depends on thread completion order, the accumulated
regrets differ run-to-run even with identical inputs.

## Verification needed

1. **Run with `--threads 1` twice.** If the results are bit-identical,
   nondeterminism is confirmed as thread-related.
2. **Run with the same thread count twice.** If they still differ, the
   bug is elsewhere (RNG seeding, HashMap iteration order, timers in
   the strategy path).

## Fix candidates

- **Deterministic merge order.** Sort batch items before accumulating.
  Cost: some CPU. Fully fixes the problem.
- **Fixed-precision accumulation.** Round each thread's partial sum
  before merging. Reduces (doesn't eliminate) the divergence.
- **Document and accept.** Report A/B deltas with an added
  "nondeterminism floor" of ±X mbb. Requires measuring X.

## Action

**Do not launch more A/Bs until this is characterized.** The 33 mbb
divergence at 5M is the current lower bound; the true floor across
full runs is unknown. Once measured, every existing result can be
re-evaluated with the correct error bar.
