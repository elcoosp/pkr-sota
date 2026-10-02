> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

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

---

## Correction (2026-10-01): the avg_power dimension was never tested

The grid above defines a 4th axis — strategy-sum weight power
(`PKR_AVG_POWER`, default 2.0) — and the Kuhn test comment claimed it
was run at `avg_power=2`. **That was wrong.** `crates/pkr-testgames/
src/kuhn.rs` accumulates its strategy sum uniformly
(`s[a] * reach[self]`, no `t^p`), so every Kuhn result in this doc was
produced with *uniform* averaging, regardless of the `avg_power`
setting. Only the `neg_floor` axis was actually varied.

Consequences:

- The Kuhn verdict ("RM+ floor helps") stands — it tested what it
  tested — but says nothing about `avg_power`.
- The NLHE A/B (`v42-vs-v43-ab.md`) held `avg_power=2` fixed on both
  sides, so it also says nothing about `avg_power`.
- Therefore "F5 is closed for both dimensions" should read "closed for
  the floor and averaging-site dimensions". **`avg_power` remains
  untested on any game.**

`avg_weight(t) = t^p` with `p=2` is standard DCFR (Brown & Sandholm),
so it is not obviously wrong. But whether `p=2` beats `p=0` (uniform)
or `p=1` (linear / CFR+) *on this abstraction* is an open empirical
question, and a plausible contributor to the ~3–6M exploitability
turn-up that v42, v43 and v45 all share: quadratic recency weighting
lets the noisiest late iterations dominate the exported average.

The harness has since been wired to read `TrainConfig::avg_power`, so
the `p ∈ {0, 1, 2}` sweep can be run on Kuhn. Treat any Kuhn result
with the same caveat as the floor grid: Kuhn may be too small to
exhibit the long-run pattern the sweep is looking for.

---

## avg_power sweep result (2026-10-01, Kuhn)

Harness wired to read `avg_power` (commit `de13676`, fixed `e896146`).

| p | 1e5 exploitability | 1e6 exploitability |
|---|---|---|
| 0 (uniform) | 0.000398 | 0.000199 |
| 1 (CFR+) | 0.000302 | 0.000129 |
| 2 (DCFR, production) | 0.000318 | **0.000093** |

**At convergence the production default (p=2) wins.** The quadratic
recency weighting is not harmful on Kuhn — if anything it converges
fastest at 1e6. This is the third dimension where the toy game
validates the current default (floor: RM+ wins; site: equivalent;
power: p=2 wins).

Kuhn is far too small to exhibit the ~3-6M NLHE turn-up, so this does
NOT close the question. It only removes the weakest form of the
hypothesis ("quadratic weighting is simply wrong"). The remaining
form — "quadratic weighting tracks late noise once the abstraction
saturates" — needs an NLHE run.

## NLHE avg_power A/B — preliminary (v46 vs v42)

v46 = `PKR_AVG_POWER=1` vs v42 = `PKR_AVG_POWER=2`, same seed/tables/
schedule, evaluated on identical deals (eval seed = iteration number).

| iter | v42 (p=2) | v46 (p=1) | delta |
|---|---|---|---|
| 3M | 3313.4 | 3231.8 | -81.5 |
| 6M | 3430.3 | 3300.5 | -129.8 |
| 9M | 3374.6 | 3275.0 | -99.6 |
| 12M | 3531.8 | 3377.3 | -154.5 |

Every paired point favors p=1 (mean -116 mbb), but:
- best-vs-best is -81.5, INSIDE the pre-registered +/-260 equivalence
  band, so the pre-registered verdict is "equivalent";
- the points are serially correlated (one run), so the consistency is
  suggestive, not a clean significance test;
- the ~3-6M turn-up persists under p=1 — the averaging weight is NOT
  the cause of the plateau.

Interpretation: p=1 may be mildly better than p=2, but neither the
magnitude nor the evidence clears the bar to switch the default.
A multi-seed paired A/B would settle it if it matters. The turn-up
remains unexplained by floor, site, power, or feature space.

## NLHE avg_power A/B — FINAL (v46 vs v42)

| iter | v42 (p=2) | v46 (p=1) | delta |
|---|---|---|---|
| 3M | 3313.4 | 3231.8 | -81.5 |
| 6M | 3430.3 | 3300.5 | -129.8 |
| 9M | 3374.6 | 3275.0 | -99.6 |
| 12M | 3531.8 | 3377.3 | -154.5 |
| 15M | 3709.4 | 3581.4 | -128.0 |
| 18M | 3780.4 | 3632.2 | -148.2 |

**Verdict (pre-registered rule): EQUIVALENT.** Best-vs-best is -81.5,
inside the +/-260 band -> keep p=2 (fewer moving parts, more
checkpoints match).

**But the paired data is suggestive:** 6/6 points favor p=1, mean
-124 mbb, and both bests are at 3M (so the best-reading comparison is
itself paired on the same deals). A paired SE is well below the
unpaired ~183, so -81.5 is arguably borderline-significant. The catch:
ONE seed. A second seed could flip it. If ~100 mbb matters, run
v47 = p=1 seed 43 vs v42 seed 43; otherwise keep p=2 and move on.

**Both curves turn up identically** (best at 3M, rising after 6M under
both p). This RULES OUT the averaging weight as the cause of the
turn-up. Four hypotheses now dead: feature space (F4), averaging site
(F5), regret floor (F5), averaging weight (this). The turn-up is either
intrinsic to the k=200 abstraction or an estimator artifact (the
in-sample BR overfitting a growing infoset table — untested).

Kuhn said p=2 wins; NLHE says p=1 is mildly better. Toy-game verdicts
do not transfer.

## avg_power seed-43 confirmation (in progress, 2026-10-02)

| 3M eval | p=2 | p=1 | delta |
|---|---|---|---|
| seed 42 | 3313.4 | 3231.8 | -81.5 |
| seed 43 | 3357.5 | 3230.0 | -127.5 |

p=1 lower on both seeds at the 3M point. Full curves (v47/v48) still
running; decide on the full-run bests, not the 3M point.

## avg_power seed-43 confirmation (in progress, 2026-10-02)

| 3M eval | p=2 | p=1 | delta |
|---|---|---|---|
| seed 42 | 3313.4 | 3231.8 | -81.5 |
| seed 43 | 3357.5 | 3230.0 | -127.5 |

p=1 lower on both seeds at the 3M point. Full curves (v47/v48) still
running; decide on the full-run bests, not the 3M point.

### seed-43 confirmation, 6M point

| iter | v47 (p=2) | v48 (p=1) | delta |
|---|---|---|---|
| 3M | 3357.5 | 3230.0 | -127.5 |
| 6M | 3332.6 | 3217.5 | -115.1 |

p=1 lower at both points on seed 43 too. Combined with seed 42, the
~100-125 mbb p=1 edge is repeatable across two seeds — not seed-42
luck. Full curves still running; decide on the full-run bests.

### seed-43 confirmation, 6M point

| iter | v47 (p=2) | v48 (p=1) | delta |
|---|---|---|---|
| 3M | 3357.5 | 3230.0 | -127.5 |
| 6M | 3332.6 | 3217.5 | -115.1 |

p=1 lower at both points on seed 43 too. Combined with seed 42, the
~100-125 mbb p=1 edge is repeatable across two seeds — not seed-42
luck. Full curves still running; decide on the full-run bests.

### seed-43 confirmation, 9M point

| iter | v47 (p=2) | v48 (p=1) | delta |
|---|---|---|---|
| 3M | 3357.5 | 3230.0 | -127.5 |
| 6M | 3332.6 | 3217.5 | -115.1 |
| 9M | 3493.8 | 3398.5 | -95.3 |

3/3 points favor p=1 on seed 43 (seed 42: 6/6). Both curves turn up
at the same place; p=1 sits ~100-125 mbb lower without fixing the
turn-up. Best so far: p=2 3332.6 @ 6M, p=1 3217.5 @ 6M (-115.1).

### seed-43 confirmation, 9M point

| iter | v47 (p=2) | v48 (p=1) | delta |
|---|---|---|---|
| 3M | 3357.5 | 3230.0 | -127.5 |
| 6M | 3332.6 | 3217.5 | -115.1 |
| 9M | 3493.8 | 3398.5 | -95.3 |

3/3 points favor p=1 on seed 43 (seed 42: 6/6). Both curves turn up
at the same place; p=1 sits ~100-125 mbb lower without fixing the
turn-up. Best so far: p=2 3332.6 @ 6M, p=1 3217.5 @ 6M (-115.1).
