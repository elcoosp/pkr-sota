> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

# SOTA A/B execution plan (from research doc, 2026-09-24)

Ordered by expected impact × implementability. Each item gets its own
commit and its own A/B. Do not bundle.

## In flight (running now)
- **α=1.5 vs α=2** — patched local commit 9b17a8f. A/B launches
  automatically after ε chain finishes (α chain, /tmp/alpha-ab-chain.log).
  Accept: v27a15 @ 20M ≤ 5450 (-5% vs v25final).

## Queue (do in this order after α result)

### 1. Alternating updates  [HIGHEST EV, structural]
- **Research claim:** simultaneous vs alternating shifts exploitability
  by "one to two orders of magnitude" (Crazyhouse/CFR+ literature).
- **Change:** in `run_iterations_parallel`, split each logical iteration
  into two half-iterations. Flush Player 0's batch, then run Player 1
  against the updated regrets, then flush Player 1's batch.
- **Env gate:** `PKR_ALT_UPDATES=1`, default off.
- **Impact on it/s:** expect -15-25% (2× flushes per iter). If quality
  gain doesn't exceed the throughput loss, reject.
- **A/B protocol:** 20M iters each, matched ε=0.01, α=1.5 (winner of
  prior A/B). Accept if expl_mbb @ 20M is ≤ 5450 by ≥ 2σ.

### 2. HS-DCFR  [small, independent]
- **Research claim:** schedule α(t), β(t), γ(t) over horizon; AAAI 2026
  SOTA. "A few lines of code."
- **Change:** add `total_iterations` to `Trainer`. In `dcfr_step()`,
  interpolate α from 2.0 → 1.5 and γ from 2.0 → 1.0 as t/T → 1.
- **Env gate:** `PKR_HS_DCFR=1`.
- **A/B protocol:** 20M iters, matched on the α=1.5 winner. Accept if
  expl_mbb @ 20M is ≤ 5400 by ≥ 2σ.

### 3. RBP pruning (fix dead FBRS)  [medium, ~30% node reduction]
- **Research claim:** regret-based pruning "squares the performance
  gain of partial pruning." In external sampling, only the traverser's
  reach matters; prune when reach × strategy[a] == 0.
- **Change:** replace the dead FBRS block in `traversal.rs` with a
  reach-probability check before recursing. Skip subtrees unreachable
  from the traverser's own strategy.
- **Env gate:** `PKR_RBP=1`.
- **A/B protocol:** 20M iters. Accept if nodes/it drops ≥ 20% AND
  expl_mbb @ 20M is within 1σ of the baseline.

### 4. AIVAT  [measurement, half-day]
- **Research claim:** 54× variance reduction on the exploitability
  estimate at matched deal count.
- **Change:** add a control variate to `sampled_exploitability`. Baseline
  value per public state must be frozen (precomputed from the blueprint
  before eval). Heuristic: EHS²-based expected value at each terminal.
- **Effect:** `--eval-deals` can drop from 5000 to ~500, making every
  future A/B 10× faster.
- **A/B protocol:** run same checkpoint × 10 at both 500 and 5000 deals,
  compare SEs. Accept if AIVAT SE at 500 ≤ raw SE at 5000.

### 5. VR-MCCFR  [research, multi-day]
- **Research claim:** 3 orders of magnitude variance reduction in the
  regret deltas themselves.
- **Change:** replace `delta = v[a] - v_sigma` with a baseline-corrected
  estimator. Requires propagating baselines up the sampled trajectory.
- **Risk:** easy to introduce bias.
- **Prerequisite:** validate on `kuhn_experiment` first. Do NOT run on
  NLHE without Kuhn evidence.

### 6. PCFR+ proper fix  [medium]
- **Change:** separate strategy computation (which uses the prediction)
  from regret update (which uses the true increment). Current code has
  prediction feeding into regret — that's the bug the earlier session
  caught.
- **Env gate:** `PKR_PCFR_PLUS=1` (replaces current `PKR_MOMENTUM`).
- **Risk:** the earlier momentum attempt poisoned our perception.
  Rebuild from the paper's PRM+ definition, not from our broken version.
- **Defer:** after HS-DCFR lands — both are discount-schedule-adjacent
  and shouldn't be A/B'd in parallel.

### 7. River subgame re-solve  [research, out of scope]
- Requires the public-tree BR walker (`public_br.rs`) to be finished.
- Not a quick experiment.

### 8. Embedding CFR  [out of scope]
- No public implementation as of the research date.
- Confirms our T2.2 failure: discrete clustering is fundamentally
  lossy. A real fix needs a continuous embedding layer + advisor
  networks. Multi-week project.

## Explicitly NOT doing
- Finer discrete river/turn buckets: T2.2 proved this fails.
- Neural CFR variants: no infra, no clear code release.
- Schedule-tuning of the broken momentum: HS-DCFR supersedes.

## Execution sequence
1. Wait for ε chain (~15 min)
2. Wait for α A/B (α chain auto-runs it, ~15 min)
3. Decide α winner
4. Prep alternating-updates patch
5. Rebuild once (α winner + ALT flag)
6. A/B ALT
7. Repeat for HS-DCFR, RBP, AIVAT in that order
