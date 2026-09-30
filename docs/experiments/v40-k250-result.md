# v40 k=250 — not a clean A/B

**Date:** 2026-09-30
**Status:** inconclusive. The configuration drifted.

## What ran

`outputs/v40-k250`, 30M iterations, seed 42, 8 threads, 5000-deal eval.

| iter | expl_mbb | SE |
|---|---|---|
| 3.0M | 3099.6 | 166.8 |
| 6.0M | 2988.6 | 160.9 |
| 9.0M | 3170.1 | 169.7 |
| 12.0M | 3429.5 | 172.5 |
| 15.0M | 2996.1 | 174.9 |
| 18.0M | 3114.2 | 174.5 |
| 21.0M | 3428.0 | 182.3 |
| 24.0M | 3108.3 | 178.6 |
| **27.0M** | **2825.1** | 174.3 |
| 30.0M | 3377.4 | 184.0 |

Best: 2825.1 mbb @ 27.0M.

## Why it's not a clean comparison

The point of this run was "does k=250 beat k=200 at the same config?"
It cannot answer that because two other variables moved at the same
time:

1. **`PKR_EXPLORE_EPSILON=0.05`.** My launcher set the historical code
   default instead of the experiment value of 0.01. That is *exactly*
   the F2 class of bug the audit called out — and I introduced it
   here.
2. **Pre-F6 game tree.** The run was launched before the F6 fix
   (jams always legal, min-raise-to clamp). Its readings are on a
   smaller legal-action set.
3. **Pre-F2 config defaults.** The binary predates the single
   `TrainConfig` source of truth, so `stats.json` records the old
   defaults (momentum=on, avg_power=2, eps=0.05), not what actually
   ran.

Any of those could explain the reading moving 300-500 mbb either way.

## What it does suggest

The best reading (2825) is *worse* than the v38 30M pool best
(2171, seed 202), and worse than the pool mean (2216.6). Even
accounting for the F2 drift and the F6 tree change, there is no
evidence here that increasing k from 200 to 250 helps.

The audit's own hypothesis was that k=200 is *coarse*, not that 250
would be enough. 250 is a 25% increase; a real test of the "bigger k"
hypothesis needs k≈1000, which the tables' `u8` width cannot express
without a format change.

## What a clean k-sweep would require

1. `TrainConfig`-based launch (done, F2).
2. Same `explore_epsilon` across arms: 0.01.
3. Post-F6 game tree across arms.
4. k ∈ {200, 250} at minimum; k=255 is the config-only ceiling.
5. Two seeds per arm to get a pooled SE.

Estimated cost: 4 runs × 30M iters × ~2.3h each = ~9h.

## Recommendation

Skip the clean k=250 A/B. 250 is too small a step to overcome
seed-level noise (SD ~40-90 mbb, but F1's estimator fix may change
the noise floor). The k dimension is not the binding constraint —
F3 (infoset key) and F4 (feature space) are.
