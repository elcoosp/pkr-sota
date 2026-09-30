# F5 — regret update and averaging grid

**Date:** 2026-09-30
**Status:** flags landed, grid not run.
**Supersedes:** the audit's F5 hypothesis (unverified).

## What the flags are

Two new `TrainConfig` fields, both default to the current behavior:

| flag | env | default | when false |
|---|---|---|---|
| `neg_floor` | `PKR_RM_PLUS` | `true` | allow negative regrets, clamp to -2^62 |
| `avg_at_traverser` | `PKR_AVG_AT_TRAVERSER` | `true` | accumulate at opponent node, `strategy · t^p` |

Combined with the already-existing flags:

| flag | env | default | meaning |
|---|---|---|---|
| `avg_power` | `PKR_AVG_POWER` | `2.0` | strategy-sum weight `t^p` |
| `sequential` | `PKR_F5_SEQUENTIAL` | `true` | fold deltas one at a time |

## The grid

2 (floor) × 2 (avg site) × 3 (weight power) × 2 (sequential) = 24 configs.

**Do not run all 24 on NLHE.** The audit's own advice: run on Kuhn or
Leduc first (tens of seconds each), then confirm a winner on NLHE
with the tournament harness.

### Phase 1 — Kuhn (fast, exact)

`pkr-testgames` has a Kuhn CFR harness. Modify it to read the same
`TrainConfig`. For each of the 24 configs:

    PKR_RM_PLUS=<0|1> PKR_AVG_AT_TRAVERSER=<0|1> \
    PKR_AVG_POWER=<0|1|2> PKR_F5_SEQUENTIAL=<0|1> \
    cargo test --release -p pkr-testgames kuhn -- --ignored

Record: convergence iteration, final exploitability, wall time.

### Phase 2 — Leduc (medium)

Same 24 configs on Leduc (if the testgames crate has it; if not, skip
this phase and go straight to a 2M-iteration NLHE screen).

### Phase 3 — NLHE screen

The 3-4 best Kuhn configs, 5M iterations each, seed 42, evaluated with
the F1-fixed `sampled_exploitability`. Only configs that beat baseline
by > 200 mbb proceed.

### Phase 4 — Tournament confirmation

The single best config vs baseline, 100k paired deals through
`pkr_fuzz::tournament`. Require `mean_diff > 0` with `t > 2`.

## What we expect

The audit's hypothesis: the RM+ floor plus quadratic weighting tracks
late noise, which would explain why exploitability rises after ~30M
iterations. F1 alone could produce that pattern.

**If F1 fixed the divergence**, expect the floor to be neutral or
mildly positive and the averaging site to be neutral. In that case,
stop here — the flags stay at their current defaults.

**If the divergence persists after F1**, expect `neg_floor=false` and
`avg_at_traverser=false` to reduce it. Measure and iterate.

## Why not just flip the flags now

Both changes affect convergence, not correctness. A wrong choice can
make training slower to converge without making it wrong. Testing
them cheaply on Kuhn/Leduc costs minutes; testing them on NLHE costs
hours. Do the cheap test first.
