> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

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

## RESOLVED: the turn-up is an estimator artifact (2026-10-01)

Deal-sensitivity of v42's 18M checkpoint, same eval-now path:

| deals | expl_mbb | SE |
|---|---|---|
| 5,000 | 3795.8 | 146 |
| 20,000 | 1707.3 | 55 |

**The reading more than halves with 4x the deals.** In-sample BR
overfitting is enormous -- far bigger than the ~300 mbb "turn-up". As
training adds infosets, the 5000-deal BR overfits them, inflating the
reported number. The rise after 3-6M is therefore substantially (likely
mostly) an artifact of the 5000-deal in-sample estimator, NOT real
policy degradation.

Implications:
- **The turn-up needs no fix.** It is a measurement artifact. Hypothesis
  B (estimator artifact) wins; hypothesis A (abstraction saturates) is
  not required to explain it.
- **Cross-run A/Bs remain valid**: both sides use the same 5000 deals
  (eval seed = iteration), so the *paired difference* is trustworthy
  even though the absolute level is inflated. v46 vs v42 stands.
- **Absolute exploitability numbers in every experiment doc are
  inflated ~2x.** They compare fine to each other at 5000 deals but
  overstate true exploitability. Use >=20000 deals for any quoted
  absolute figure (matches the earlier arena hand-count lesson).
- **`--eval-now` uses a DIFFERENT seed** (`EVAL_SEED ^ iter`) than the
  in-loop eval (`iter` as seed), so eval-now readings don't match the
  training curve. Both are inflated; do not mix them.

### Correction / limitations on the above (2026-10-02)

The resolution is directionally right but I overclaimed in two places:

1. **"Inflated ~2x in every doc" is from ONE checkpoint.** The 3795 ->
   1707 drop was measured on v42's 18M model only. The inflation factor
   is a property of the infoset table size, so it almost certainly
   varies run to run and grows with iteration within a run. Do not
   apply a flat 2x to other numbers without measuring.

2. **The mechanism predicts the turn-up should SHRINK at high deal
   counts.** Early checkpoints have fewer infosets -> less in-sample
   overfitting -> less inflation; late checkpoints have more -> more
   inflation. So at 20k+ deals the early/late gap should be smaller
   than the ~300 mbb seen at 5000. If it reverses, the true curve
   *descends*, and "best at 3-6M" is itself an artifact.

   This is untestable with surviving artifacts: only FINAL checkpoints
   survive (train.ckpt is overwritten each checkpoint; the 3M model
   exists only as blueprint.bin, the average-strategy format the eval
   cannot load as a regret table). A future run should checkpoint to
   distinct paths to enable it.

3. **20k may not even be converged.** A 40k-deal eval is running
   (/tmp/eval40k-result.txt). If 40k reads much lower than 20k's 1707,
   the "true" value is still below 1707 and even 20k is inflated.

What still holds with confidence: the 5000-deal reading is
substantially inflated, so the ~3-6M turn-up is at least partly (likely
mostly) an estimator artifact. Cross-run paired A/Bs at 5000 deals
remain valid (same deals both sides).

---

## Bug found during the 2026-10-02 hunt: exploit shifter misreads the CDF

`crates/pkr-exploit/src/lib.rs::apply_archetype_shifts` decoded
`SotaAdvice::cdf_probabilities` as a raw per-action vector
(`byte / sum`), but it is a CUMULATIVE cdf everywhere else
(`writer::quantize_cdf`, `pkr_fuzz::decode_cdf_into`, `reader.rs` all
differencing successive bytes).

Effect: a [64,128,192,255] uniform CDF (true probs [.251,.251,.251,.247])
was read as [.10,.20,.30,.40]. Even the no-op `Balanced` archetype
rewrote it to [26,77,154,255], so ANY call to `apply_shifts` silently
corrupted the strategy, and the ±MAX_DEVIATION cap could not catch it
because the deviation was measured in the wrong space.

Its two tests missed it: one asserts only `.len`, the other
short-circuits at the recenter interval. Fixed by difference-decoding;
added `test_exploit_shifter_balanced_roundtrips_cdf`.

---

## Bug found during the 2026-10-02 hunt: exploit shifter misreads the CDF

`crates/pkr-exploit/src/lib.rs::apply_archetype_shifts` decoded
`SotaAdvice::cdf_probabilities` as a raw per-action vector
(`byte / sum`), but it is a CUMULATIVE cdf everywhere else
(`writer::quantize_cdf`, `pkr_fuzz::decode_cdf_into`, `reader.rs` all
differencing successive bytes).

Effect: a [64,128,192,255] uniform CDF (true probs [.251,.251,.251,.247])
was read as [.10,.20,.30,.40]. Even the no-op `Balanced` archetype
rewrote it to [26,77,154,255], so ANY call to `apply_shifts` silently
corrupted the strategy, and the ±MAX_DEVIATION cap could not catch it
because the deviation was measured in the wrong space.

Its two tests missed it: one asserts only `.len`, the other
short-circuits at the recenter interval. Fixed by difference-decoding;
added `test_exploit_shifter_balanced_roundtrips_cdf`.

### 40k-deal check: 20k is NOT converged (2026-10-02)

| deals | expl_mbb | SE |
|---|---|---|
| 5,000 | 3796 | 146 |
| 20,000 | 1707 | 55 |
| 40,000 | **1222** | 35 |

The reading is still falling steeply at 40k (-485 from 20k). So:
- **20k is not a converged estimate** either; every absolute number in
  the docs is inflated by an unknown, still-shrinking factor.
- The decline does NOT fit `bias ~ c/deals`: that law predicts ~1707 at
  40k, but we measured 1222. The estimator's bias decays more slowly
  than the naive in-sample-overfit model, or the true value is well
  below 1222.
- Practical rule: for a QUOTED absolute exploitability, use >=40k deals
  AND report it as an upper bound. For A/B decisions, keep using the
  same deal count on both sides (paired) -- that remains valid.

### 40k-deal check: 20k is NOT converged (2026-10-02)

| deals | expl_mbb | SE |
|---|---|---|
| 5,000 | 3796 | 146 |
| 20,000 | 1707 | 55 |
| 40,000 | **1222** | 35 |

The reading is still falling steeply at 40k (-485 from 20k). So:
- **20k is not a converged estimate** either; every absolute number in
  the docs is inflated by an unknown, still-shrinking factor.
- The decline does NOT fit `bias ~ c/deals`: that law predicts ~1707 at
  40k, but we measured 1222. The estimator's bias decays more slowly
  than the naive in-sample-overfit model, or the true value is well
  below 1222.
- Practical rule: for a QUOTED absolute exploitability, use >=40k deals
  AND report it as an upper bound. For A/B decisions, keep using the
  same deal count on both sides (paired) -- that remains valid.
