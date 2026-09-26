> **ARCHIVED — superseded by** [`docs/experiments/variance-reduction-negative.md`](../experiments/variance-reduction-negative.md).
> Empirical verification on a converged blueprint showed `rho ~= 0`
> (no control-variate benefit) and a bias in the antithetic hole-card
> swap. This document is kept for its derivation and citation of
> Burch 2018, but its 'implement Tier 1 + Tier 2 today' recommendation
> was tested and does not hold on this codebase. See the negative-result
> doc for what to do instead (common random numbers).

---

# AIVAT-Style Variance Reduction: Implementation Plan

## Bottom line up front

Yes — this is implementable today, and it will materially change your A/B cadence. Three tiers, each a strict improvement. I recommend doing Tier 1 first (30 min), verifying it, then Tier 2 (2–3 h) if Tier 1 goes green. Tier 3 is the paper's full AIVAT and is a multi-day project.

The core insight: **your evaluation already knows one player's strategy** — the blueprint is fixed, deterministic, and stored in the CFR table. That's the exact precondition AIVAT needs. You don't need to learn a value function; you can use the blueprint's own self-play EV as the control variate.

---

## What AIVAT does (the mechanism)

From Burch et al. 2018: every terminal outcome `z` has a raw value `v(z)`. AIVAT replaces it with:

```
AIVAT(z) = [Σ_{z'∈W} π_Pa(z') v(z')] / [Σ_{z'∈W} π_Pa(z')] + Σ_{H∈ℋ} k_H(z)
```

where `Pa` is the set of players with **known** strategies (including chance), `ℋ` partitions known-player decision points, and the correction term is:

```
k_H(z) = [Σ_a Σ_h π_Pa(h·a) u_h(a)] / [Σ_h π_Pa(h)]  
       − [Σ_h π_Pa(h·a_O) u_h(a_O)] / [Σ_h π_Pa(h·a_O)]
```

Each correction term has expected value zero, so the whole estimator is **provably unbiased** regardless of the heuristic `u_h(a)`. The base value uses imaginary observations over private information.

The HUNL results: **68% SD reduction** with one known player, **99.8%** in the Leduc both-known case. GTO Wizard reports a 3× SD reduction = **10× fewer hands** for equivalent significance.

---

## Why this maps cleanly to your codebase

Three properties of `pkr-exploit::best_response::sampled_exploitability` make it a natural fit:

1. **The blueprint strategy is known.** Every infoset visited during the BR walk has a corresponding entry in the CFR table. `Pa = {chance, blueprint}`.
2. **The BR strategy is deterministic given the blueprint.** The BR walk takes `argmax_a cfv(a)` at every BR node, so `Pa` can also include the BR player for free.
3. **Your abstraction buckets ARE the partition H.** `KMeansAbstraction::get_infoset_hash` already maps every `(hole, board, history, street)` to a u8 cluster id. All states in the same cluster are indistinguishable to the opponent — exactly the condition AIVAT's ℋ partition requires.

You don't need new data structures. You need a way to compute the correction terms during the eval walk.

---

## Tier 1 — Antithetic sampling (30 min, low risk)

**Idea:** For each deal `(h0, h1, board)`, also evaluate the swapped orientation `(h1, h0, board)`. Average the two BR values. In a zero-sum game, card luck partially cancels between orientations.

**Cost:** 2× eval time. **Benefit:** 2–3× SD reduction if the correlation is −0.5 to −0.7.

**Implementation sketch** — one function change in `pkr-exploit/src/best_response.rs`:

```rust
// Before:
let sample = br_value(h0, h1, &board);
samples.push(sample);

// After:
let v_a = br_value(h0, h1, &board);
let v_b = br_value(h1, h0, &board);
samples.push(0.5 * (v_a + v_b));
```

**Why this works:** in a symmetric zero-sum game, `v(h0,h1,board) + v(h1,h0,board) ≈ 2 * E[v]`, so the average is closer to the true expectation than either individual sample.

**Risk:** the BR strategy depends on hole cards, so `v_a` and `v_b` use different BR strategies. The correlation is still strongly negative but not exactly −1. Verify empirically before trusting.

---

## Tier 2 — Baseline control variate (2–3 h, high value)

This is the **Baseline** method (Davidson et al. 2013) and the highest-value AIVAT-family technique for your setup. From the paper:

> *"The baseline method leverages the self play of any available agent to produce a control variate for variance reduction."*

The estimator is:

```
Z_i = X_i + c(Y_i − E[Y])
```

where `X_i` = BR value for deal `i`, `Y_i` = **blueprint's self-play value for the same deal**, and `E[Y] = 0` by symmetry. The optimal coefficient is `c* = −Cov[X,Y]/Var[Y]`.

**Implementation:**

1. **Add a `blueprint_ev_walk` function** to `best_response.rs` that walks the tree with the blueprint's strategy on both sides (no `max`, just `Σ_a σ(a) * cfv(a)`). This is ~50 lines, mirroring the existing BR walk.

2. **In the sampling loop**, for each deal, compute both `br_value` and `blueprint_ev`:
   ```rust
   let x_i = br_value(h0, h1, &board);
   let y_i = blueprint_ev(h0, h1, &board);
   ```

3. **Accumulate sample statistics**, then apply the control variate:
   ```rust
   let c = -cov_xy / var_y;  // or fix c=1 for the first cut
   let z_i = x_i - c * y_i;
   ```

4. **Report SE from the corrected samples.**

**Expected reduction:** The paper reports Baseline is *competitive with duplicate* in 2p poker. In your case, `X` and `Y` are both dominated by the same card luck, so the correlation should be high (0.7–0.9), giving `1 − ρ² ≈ 0.2–0.5` variance ratio → **1.4–2.2× SD reduction**.

**Combined with Tier 1:** Tier 1 gives ~2.5×, Tier 2 gives ~1.8× on top → **~4.5× total SD reduction**. That takes your SE from 185 mbb at 4000 deals to ~40 mbb, which means you can detect 100 mbb A/B differences instead of 400 mbb.

**Cost:** one extra tree walk per deal (same complexity as the BR walk, ~2× eval wall time).

---

## Tier 3 — Full AIVAT (multi-day, defer)

The paper's method adds:
- **Action corrections** using the BR walk's already-computed `cfv(a)` per action at each visited node — essentially free if the walker records them.
- **Imaginary observations** over private hands in the partition `H` — this is the expensive part, requiring a sum over all compatible opponent hole cards at each node.
- **Counterfactual value storage** during training so the value function is the blueprint's own values, not a hand-crafted heuristic.

The paper's full AIVAT gives the 68–99% reductions. Baseline alone gives ~40–50%. The gap is real but requires ~2 weeks of careful work.

**Defer Tier 3 until Tier 1+2 have been validated on a live A/B.** The marginal gain over Baseline is large but the engineering risk is much higher.

---

## Verification plan (do this today)

After Tier 1 or Tier 2 lands:

1. **Fix a checkpoint** — use `outputs/v34long/blueprint.best.bin` (the 2526 mbb shipped blueprint).
2. **Run the eval twice** with the same seed:
   - Once with the old estimator (raw BR values)
   - Once with the new estimator
3. **Compare the reported `expl_std_err_mbb`.** It should drop by the predicted factor.
4. **Cross-check with independent seeds:** run both estimators 5 times with different `EVAL_SEED` values and compute the empirical SD of the 5 mean estimates. This is the ground-truth variance.
5. **Sanity: the mean should not shift.** If the corrected mean differs from the raw mean by more than 2× the raw SE, something is wrong (bias or implementation bug).

Write the verification result as `docs/experiments/aivat-variance-reduction.md` with the measured SD ratio. That becomes the justification for using the corrected estimator in the overnight run.

---

## Expected impact on the overnight A/B

Your current cadence:
- 4000 deals × ~10 min = SE ~185 mbb
- Detects differences > 370 mbb at 2σ

With Tier 1+2:
- Same 4000 deals, SE ~40–60 mbb
- Detects differences > 100–120 mbb at 2σ

**That's a 3–4× improvement in A/B resolution at the same wall-clock cost.** For the overnight run, you could either:
- **Halve the eval deals** (2000 instead of 4000) and keep the same SE, saving 5 min per eval point
- **Keep 4000 deals** and gain 3× more statistical power
- **Run two A/Bs** (e.g., river-rich AND flop-13D) in the same overnight window

My recommendation: keep 4000 deals, gain the power. The overnight run's purpose is to detect whether the next feature axis helps; the extra power makes the answer more trustworthy.

---

## What I need from you to write the exact patch

I don't have `crates/pkr-exploit/src/best_response.rs` in the conversation. If you paste it (or just the `sampled_exploitability` function and the BR walker signature), I can write:

- The exact diff for Tier 1 (5 lines)
- The exact diff for Tier 2 (a new `blueprint_ev_walk` function + the sampling loop change)
- The verification script that runs both estimators on `v34long/blueprint.best.bin` and reports the SD ratio

The pipeline is still running Run A at ~8M iters; nothing is blocked by waiting for `best_response.rs`.
