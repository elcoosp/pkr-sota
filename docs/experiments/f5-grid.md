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

---

## Kuhn result (2026-09-30)

First grid run. `crates/pkr-testgames/src/kuhn.rs`,
`f5_grid_tests::kuhn_floor_grid`.

| iters | neg_floor=true (RM+) | neg_floor=false (DCFR beta) |
|---|---|---|
| 1e5 | 0.000398 | 0.000338 |
| 1e6 | **0.000199** | 0.000323 |

**Verdict on Kuhn: the RM+ floor helps.** At 1e6 iterations,
`neg_floor=true` converges to 0.000199 exploitability while
`neg_floor=false` stalls at 0.000323. The RM+ floor isn't the
convergence-inhibitor the F5 hypothesis suggested.

### What this means

The audit's F5 hypothesis was: "flooring high-variance sampled
regrets biases them upward, which would explain the exploitability
rise after ~30M on NLHE." Kuhn is too small to exhibit that pattern;
almost any update rule converges.

But the direction is *against* the hypothesis. Flipping the floor off
made convergence worse on Kuhn, not better. If the same pattern holds
on NLHE, the F5 flags should stay at their current defaults
(`neg_floor=true`, `avg_at_traverser=true`).

### What still needs testing

- **Leduc.** Larger than Kuhn, still cheap. If the RM+ advantage
  persists there, F5 is effectively closed for the floor dimension.
- **NLHE at 30M+.** The place where the original symptom appeared.
  That's what the flags are actually for. But F1's fix may have
  already removed the symptom — the divergence the audit attributed
  to sampling could have been the estimator bug.

### Recommendation

Given the Kuhn result, keep `neg_floor=true` as the default. Do not
flip it for the next training run. If v42 completes and its
exploitability curve is clean (no rise after ~20M), F5's floor
dimension is settled.

The `avg_at_traverser` flag has not been tested yet. That's the other
half of F5 and the one whose theory is on stronger ground (the
current scheme adds a reach factor that standard external-sampling
averaging omits).
