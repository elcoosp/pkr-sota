# Finding: universal 160M+ exploitability spike (2026-09-25)

Every 200M run in the overnight batch showed a systematic +500-800 mbb
increase in exploitability between 140M and 160M iterations, independent
of the algorithmic flag tested (ALT_UPDATES, HS-DCFR, α=2, α=1.5).

| iter | v29alt | v29rb250 | v29t22 | v29fb250 | v29hs | v29a2 |
|------|--------|----------|--------|----------|-------|-------|
| 140M | 5728 | 5672 | 5625 | 5614 | 5640 | 5873 |
| 160M | 6200 | 6209 | 6024 | 6187 | 6128 | 6114 |
| 180M | 6356 | 6279 | 6278 | 6477 | 6279 | 6339 |
| 200M | 6257 | 6309 | 6303 | 6229 | 6384 | 6344 |

All six runs (including the α=2 control, which is essentially baseline)
spike at the same iteration range. This rules out any single flag as
the cause.

## Hypotheses

1. **CFR divergence at high iteration count.** max|r| reaches 2.14e6 by
   200M. Not saturated (R_MAX = 2.3e18), but the regret accumulation
   may be unstable. Strategy oscillates rather than converges.
2. **Eval methodology.** 2000-deal BR walker has SE ≈ 330 mbb. A
   universal +500-800 jump cannot be explained by SE alone.
3. **Checkpoint/promotion interaction.** All runs use `promote-gate=3.0`.
   If the best reading was at 120M, promotion stopped and later evals
   read in-memory tables. But this doesn't explain the spike; the spike
   is in the reading itself.

## Recommendation

**Stop all future training at 120M iterations.** v25final's floor was
5450 @ 120M. Longer training makes the strategy worse on this
abstraction, regardless of config.

If we want to push the ceiling, the abstraction is the binding
constraint — the opponent-pool reframing (see
docs/roadmap/novel-directions.md) is the only credible path forward.

## Not a bug (probably)

If this were a bug in the trainer, we'd expect at least one run to
escape it. All 6 runs (2 different alphas, 4 different flags, 3
different abstractions) show identical shape. More likely: the CFR
algorithm itself destabilizes at this iteration count on this problem.
