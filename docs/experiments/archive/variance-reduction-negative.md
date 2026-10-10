> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

# AIVAT-family variance reduction — negative result

**Date:** 2026-09-26
**Status:** Negative. Code reverted; verification logs at `/tmp/pkr-vr-verify/keep/`.
**Context:** `docs/handoff/HANDOFF_2026-09-25.md` §4 lists AIVAT for eval
speedup as the highest-leverage future work. This experiment tests a cheap
subset and shows it does not transfer to a converged blueprint.

## What we tried

Two complementary variance-reduction methods applied to
`pkr_exploit::best_response::sampled_exploitability`:

1. **Antithetic hole-card pairing.** For each seed, evaluate the deal
   `(h0, h1, board)` and its transpose `(h1, h0, board)`; use the
   pair-mean as the per-deal value.
2. **Baseline control variate** (Davidson et al. 2013). For each deal,
   compute `y_i` = blueprint self-play P0 EV (`E[y] = 0` by game
   symmetry). Fit `c = Cov(x,y)/Var(y)` on the sample; estimate the
   mean from `z_i = x_i - c*(y_i - ybar)`.

Both were gated by `PKR_EVAL_VR=1` and default OFF. The legacy estimator
was unchanged.

## Verification

Ran `pkr-trainer --eval-now --eval-deals 1000` twice on
`outputs/v34long/train.ckpt` (100M-iteration checkpoint), once with VR
off and once with VR on. Same eval seed. Single run each.

| metric | VR off | VR on |
|---|---|---|
| expl_mbb | 7480.60 | 10635.18 |
| expl_stderr_mbb | 490.57 | 661.92 |
| br0 | 17.61 | 24.65 |
| br1 | 12.31 | 17.89 |

VR diagnostics from the on-run (indented block):

    VR-DEBUG: n_pairs=500 x_mean=42.5407 y_mean=-1.7333 c=-0.0577
              rho=-0.028 raw_se=662.18 vr_se=661.92 sd_ratio=1.000

## Why it failed

### Failure 1 — control variate correlation is zero

`rho = -0.028` means the blueprint self-play EV has essentially no linear
relationship with per-deal BR exploitability on this checkpoint. With
`rho ~= 0`, the control variate contributes nothing: `sd_ratio = 1.000`.

The Davidson et al. paper reports large reductions on **exploitable**
baselines. Our blueprint at 100M iterations (~2500-3000 mbb abstract
exploitability) is close enough to its own equilibrium that the
strategic signal the CV was supposed to capture has already been
absorbed. This matches the paper's own caveats about the method's power
decaying as the reference agent strengthens.

### Failure 2 — antithetic pairing is biased in HUNL

The estimator's mean shifted by `+3155 mbb`, far outside the ~500 mbb
combined noise band. Root cause: HUNL is **seat-asymmetric**. Seat 0 is
SB and acts first preflop; seat 1 is BB and closes. Swapping hole cards
between seats changes the expected value of the deal.

Concretely: `br0_orig = 17.61`, and `br0_trans = 2*24.65 - 17.61 = 31.69`.
The transposed deal has a systematically higher BR value. Averaging the
two produces a biased estimate.

A correct antithetic for HUNL would also swap positions and re-run the
tree from the SB-perspective — essentially a parallel game tree. Even
then, the CV problem above means `sd_ratio ~= 1` regardless.

## What would actually reduce eval variance

### Common random numbers across A/B arms (CRN)

The cheapest intervention: **use the same eval deal seeds for both arms
of an A/B**. Currently `pkr-trainer` computes the eval seed as:

    EVAL_SEED ^ (done as u64)   // iteration-dependent

Changing that to just `EVAL_SEED` makes every eval read the *same* deals
across both arms and across iterations. The per-reading SE stays ~185 mbb,
but the difference between two arms at matched iterations inherits the
paired-sample variance reduction. For two evals on the same deals with the
same abstraction, the correlation of `delta_expl` across arms should be
high (0.5-0.8 in the paper's HUNL setups).

The change is 1 line in `main.rs`, does not touch the estimator, and does
not affect any individual reading's reported SE. It only tightens the
**A/B delta**.

Caveat: CRN changes the meaning of the `EVAL_SEED` constant, which is
baked into golden vectors in the trainer tests. Changing it requires
regenerating those goldens and re-running any A/B whose conclusions were
drawn from an iteration-varying seed. Given the current state (no
production A/B is blocked on eval-seed stability), the switch is safe.

### Full AIVAT (Tier 3)

Worth doing only if the blueprint is re-trained from scratch on a weaker
starting configuration, where `rho` is meaningfully non-zero. Given the
Failure 1 measurement, expected gain is 1.5-2x at best, for 2 weeks of
work. **Low EV on the current setup.**

## Recommendation

1. **Do not retry AIVAT-family methods** on a converged blueprint. The
   negative result is specific to blueprint strength, not to the method.
   If we later train a weaker agent (e.g. for opponent-modeling purposes),
   re-evaluate then.
2. **Adopt CRN as the default eval protocol** for future A/B runs.
   One-line change, no estimator change, material A/B-delta tightening.
3. **Keep the ~185 mbb per-arm SE** as the working assumption for
   single-arm readings. CRN does not reduce it — only the A/B delta.

## Artifacts

- Verification logs: `/tmp/pkr-vr-verify/keep/{off,on}.log` (ephemeral).
- Original plan: `docs/archive/aivat-plan.md` (superseded banner added).
- Code: reverted. No VR commits in git history (patches applied to the
  working tree and rolled back via `git checkout --`).
- Handoff context: `docs/handoff/HANDOFF_2026-09-25.md` §4.


## Second attempt: stratified sampling by preflop hand strength

After the baseline-CV failure above, a second variance-reduction method was
tried: **stratify the deal sample by P0's preflop hand strength**. The
intuition: weak hands systematically produce low BR values and premium
hands produce high ones, so binning by hand class should reduce within-
stratum variance.

### Implementation

- `preflop_strength(a, b) -> i32`: hand-tuned score (pairs > suited >
  offsuit, high-card bonus).
- Sort all 1326 preflop combos by strength.
- For `k` strata, draw `deals/k` P0 holes from each strength band; P1 and
  board are drawn uniformly.
- Same BR walker; same report.

Gated by `PKR_EVAL_STRATIFY=K`. Default off.

### Verification

Same checkpoint, 4000 deals, both paths. Ran at 15:46 and 15:56 (while
v35 Run B was completing in the background — mild CPU contention but no
functional impact).

| metric | baseline (k=1) | stratified (k=10) |
|---|---|---|
| expl_mbb | 3401.43 | 3302.99 |
| expl_stderr_mbb | 225.40 | 211.34 |

- SE ratio: **1.066x** (225.40 / 211.34)
- Mean shift: +98.4 mbb, 0.4 sigma

### Verdict

Marginal. The theoretical prediction was 1.4-1.8x; we got 1.07x. Preflop
hand strength explains very little of the per-deal BR variance — the
postflop runout dominates. This is consistent with the baseline-CV result
(`rho = -0.028`): the exploitable-value signal is not concentrated at
preflop.

### Why this closes the thread

Two independent approaches converge on the same conclusion: **most of the
eval variance is not where we thought it was**. Neither hand structure
(preflop) nor blueprint EV captures the per-deal exploitability signal at
a converged checkpoint. Attempts to reduce variance by projecting onto
cheap features fail because the cheap features are nearly uncorrelated
with the target.

The only remaining option is full AIVAT with imaginary observations
(3-5 days of work, ~40-60% SD reduction per the Burch 2018 paper). Given
the `rho = -0.028` result, that reduction may not materialize here either.
Recommend deferring until the blueprint is weaker (i.e. re-train an
earlier checkpoint for testing) or a production need forces it.

### What we actually adopted

Nothing in the estimator. The one-line CRN change described above
(shared eval deal seeds across A/B arms) remains the only actionable
follow-up. It does not reduce per-arm SE but tightens A/B deltas
directly, which is what most experiments actually need.

### Second-attempt artifacts

- Verification stdout: see the trainer logs at 15:46 and 15:56 in the
  session transcript. CSVs were not written because `--eval-now` prints
  to stderr and skips the CSV writer (pre-existing behavior).
- Code: reverted. No stratified commits in git history.
