# The ~3-6M exploitability turn-up — consolidated investigation

**Status:** open. Four hypotheses eliminated, one test in flight.

## The phenomenon

Every post-audit NLHE run has its lowest exploitability at the
**3-6M iteration** eval, then rises. Same shape across v42, v43, v45,
v46 despite different feature spaces and update rules.

| run | best | @iter | last | slope/1M |
|---|---|---|---|---|
| v42 (p=2) | 3313.4 | 3M | 3780.4 | +31.7 |
| v43 (site) | 3402.7 | 6M | 3723.7 | +22.3 |
| v45 (feat) | 3302.3 | 6M | 3612.5 | +17.0 |
| v46 (p=1) | 3231.8 | 3M | 3632.2 | +13.7 |

**Min lands in the first two evals in 4/4 runs** (p~0.012 under
flatness). But the per-run first->last rise is only z ~ +0.8 to +2.4
(serially correlated), so "the turn-up" is *weakly* significant per
run. The robust fact is narrower: **training past 3-6M does not
improve the reading.**

## Hypotheses eliminated

| # | hypothesis | test | result |
|---|---|---|---|
| 1 | feature space (EHS/EHS^2 vs mean/potential) | F4 / v45 | equivalent; turn-up persists |
| 2 | averaging site (traverser vs opponent) | F5 / v43 | equivalent; turn-up persists |
| 3 | regret floor (RM+ vs DCFR beta) | F5 Kuhn grid | RM+ helps on Kuhn, not the cause |
| 4 | averaging weight (p=0/1/2) | v46 | equivalent per rule; turn-up persists |

## Survivors

- **A. Intrinsic to the k=200 abstraction.** The abstraction saturates
  at 3-6M; more iterations add variance, not resolution.
- **B. Estimator artifact.** The BR is fit IN-SAMPLE on 5000 deals. As
  training adds infosets (~1.2% -> 2.1%), the BR has more parameters to
  overfit the SAME deals, so the reported number could rise even if the
  policy is unchanged or improving. Untested.

## What would settle it

- **B:** eval a fixed checkpoint at 5k vs 20k deals. Flat => B is dead.
  Sharp drop => B survives. (Running: /tmp/overfit-test-result.txt.)
- **A:** requires a larger abstraction (k > 200) or a held-out eval.
  Held-out was tried and reverted (policy doesn't transfer across deal
  sets because the hash depends on cluster_id + board).

## Confound to remember

The promote gate (`--promote-min-sigma 2`, ~256 mbb) means the saved
artifact is often NOT the best reading (v45: reading 3302 @ 6M, saved
3392 @ 3M). A/B decisions must use READINGS; shipping uses ARTIFACTS,
and those can differ by ~100 mbb. See f4-abstraction-rebuild-plan.md.
