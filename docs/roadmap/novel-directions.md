# Novel directions for M1-constrained competitive HUNL

**Status: brainstorm, 2026-09-24. Not yet scheduled.**

## The reframing

We've been minimizing abstract exploitability — a Nash metric.
Real opponents are not Nash. Given M1 constraints, minimizing
exploitability against the whole strategy space is exponentially
harder than minimizing EV against a *specific* opponent.

The honest pivot: **abandon Nash pursuit, pursue opponent-specific
exploitation**.

- Nash pursuit on M1: 2-3 orders of magnitude short on compute.
- Exploit pursuit on M1: needs to beat 4-8 common archetypes,
  each of which converges in a fraction of the compute.

## Idea #1 — Opponent-pool training with classifier + switching policy

Train N specialized blueprints, each against a scripted archetype
(nit, TAG, LAG, station). At play time, an online classifier selects
the matching blueprint.

**Cost:** 4 archetypes × 30M iters = ~1.5h. Classifier is a decision
tree, minutes to train.

**Why it wins:** specialized training against a fixed strategy
converges 10-20x faster than Nash training. Real opponents cluster
into archetypes. A TAG-specialized blueprint beats a TAG human by
50-200 bb/100; a Nash-approximating bot barely does.

**Novel angle:** the switching policy — a meta-strategy that
interpolates between blueprints based on classifier confidence.
Not seen in poker literature.

## Idea #2 — Depth curriculum with cross-depth transfer

Train 10bb → 20bb → 40bb → 80bb → 200bb, each initialized from
the previous (rescaled). Shallow trees converge in minutes and
provide strong priors for deeper levels.

**Cost:** each level ~30 min; total ~2.5h + final refinement.

**Novel angle:** the rescaling function between depths. Going
20bb → 40bb isn't a scale factor — positions and bet sizings
interact with stack depth in complex ways. No precedent for HUNL.

## Idea #3 — Inference-time river solving with blueprint as prior

At play time, run a local CFR solve of the river subgame, using
the blueprint's turn strategy as the prior range. Solves in <100ms.
Only fires on river (~20% of decisions).

**Cost:** 1-2 weeks to build and validate.

**Novel angle:** use the blueprint's *regret magnitudes* (not just
strategy) as the prior, so the solver knows confidence, not just
the strategy point estimate.

## Idea #4 — Soft bucket assignment at inference (quick win)

Randomize near-boundary hands between buckets. 10 lines of code,
no retraining. Expected 2-5% gain, cheap to test.

**Novel angle:** hard clustering for training, soft inference.
Uncommon in the literature.

## Concrete sequence

**Week 1:** finish overnight A/Bs; pick branch; pivot objective.
**Week 2:** opponent-pool framework + scripted archetypes.
**Week 3:** switching policy + evaluation vs each archetype.
**Week 4-5:** inference-time river solver.
**Week 6-8:** depth curriculum + iteration on pool.

## Honest expected outcomes

- vs scripted archetypes: >300 bb/100 (winning)
- vs weak humans: 50-150 bb/100 (clearly winning)
- vs competent humans: roughly break-even
- vs top bots/solvers: still losing

The reframing trades Nash optimality for practical exploitability.
That trade is worthwhile given M1 constraints.
