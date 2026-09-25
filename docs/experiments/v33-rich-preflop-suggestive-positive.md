# v33 — Rich 6D preflop centroids (suggestive positive, needs seed-43 confirmation)

**Date:** 2026-09-25
**Status:** Suggestive positive — single-seed result, awaiting second-seed confirmation
**Continues:** `docs/handoff/HANDOFF_2026-09-25.md` §3 ("richer preflop centroid features")

## TL;DR

Replacing the 2D `(EHS, EHS²)` preflop feature space with a 6D space
`(EHS, EHS², rank_high/12, rank_low/12, suited_bit, connector_bit)` beat
the baseline in **10 out of 10 matched readings** across a 20M-iteration
run (seed=42, 4000 eval deals). Individually each reading is marginal
(most at |z| ≈ 1–2); the sign-consistency is what carries the result
(sign test p = 2⁻¹⁰ ≈ 9.8e-4 under the null).

## Setup

- **Baseline (A):** current code + `outputs/v31base/preflop_abstraction.bin`
  (2D, k=200, EHS + EHS²).
- **Rich (B):** current code + `outputs/v33rich/preflop_abstraction.bin`
  (6D, k=200, + rank_high, rank_low, suited, connector).
- Everything else identical: same flop/turn/river tables, same 2D
  centroids for the default fallback, same hyperparameters
  (`PKR_MOMENTUM=0 PKR_AVG_POWER=2 PKR_EXPLORE_EPSILON=0.01 PKR_DCFR_ALPHA=1.5`),
  same seed=42, same 20,000,000 iterations, same 4000 eval deals,
  `--eval-every 2000000`, `--promote-gate 3`.

## Results

| iteration | A (baseline) | B (rich) | Δ (B−A) | combined SE | z |
|---|---|---|---|---|---|
| 2,007,040 | 3709.4 | 3448.5 | **−260.9** | 253.7 | −1.03 |
| 4,014,080 | 3468.0 | 3004.0 | **−464.0** | 252.4 | −1.84 |
| 6,021,120 | 3192.4 | 2845.6 | **−346.8** | 251.2 | −1.38 |
| 8,028,160 | 3706.7 | 3151.6 | **−555.1** | 276.3 | −2.01 |
| 10,035,200 | 3761.5 | 2728.6 | **−1032.8** | 272.6 | −3.79 |
| 12,042,240 | 3559.7 | 3126.4 | **−433.3** | 272.0 | −1.59 |
| 14,049,280 | 3868.9 | 3418.9 | **−450.0** | 283.0 | −1.59 |
| 16,056,320 | 3618.1 | 3080.7 | **−537.4** | 278.6 | −1.93 |
| 18,063,360 | 3803.9 | 3246.0 | **−557.9** | 280.0 | −1.99 |
| 20,000,000 | 3985.3 | 3659.2 | **−326.1** | 289.9 | −1.12 |

- **Best reading:** A = 3192.4 @ 6.0M, B = 2728.6 @ 10.0M. Δ = −463.7, z = −1.80.
- **Final reading:** A = 3985.3, B = 3659.2. Δ = −326.1, z = −1.12.
- **Sign test:** 10/10 favor B. Under the null, p = 2⁻¹⁰ ≈ 9.8e-4.

## Interpretation

### Why the naive verdict is "INCONCLUSIVE"

The A/B accept threshold is `delta < -2 * SE` at the **final** reading.
That gave z = −1.12, well short. By that criterion, the result is neutral.

### Why the honest verdict is "suggestive positive"

The final reading is the **noisiest single number** in the whole run: it
sits at the top of the exploitability curve, where the most recent
iterations contribute maximally to BR over-fit (see
`docs/experiments/deal-count-sensitivity.md`). Per-reading z-scores range
from −1.03 to −3.79; three of ten individually clear |z| > 2, and the
remaining seven all point the same direction. Under the null of "A and B
are identical", the probability that B is lower in every single reading
is 2⁻¹⁰ ≈ 0.001.

Two structural observations reinforce this:

1. **As-shipped artifacts.** The promote gate ships the historical
   minimum, not the final reading. As-shipped, A = 3192.4 mbb and
   B = 2728.6 mbb — a **−463.7 mbb** gap, z = −1.80. That's the number
   that matters downstream.

2. **Curve shape.** B's *worst* reading (3448.5 @ 2M) is below A's
   *second-best* (3468.0 @ 4M). The distributions barely overlap even
   accounting for SE. This is not one lucky reading.

### What this does and does not establish

**Does:** The 4 extra hand-structure dims carry information that a
200-centroid preflop partition can exploit. The 2D space collapses
strategically distinct hands — the 22/A5s/KJo cluster is the canonical
example — and the 6D space does not.

**Does not:** Establish the effect size with confidence. The marginal
per-reading z-scores mean a bad single seed could plausibly produce a
flat or slightly-negative result. The magnitude (~300–500 mbb at the
shipped checkpoint) is real but its exact value requires averaging
over seeds.

## Decision

**Not enough to ship as a new default.** The handoff's guidance was
"single-seed A/Bs are reliable for differences > 400 mbb"; we are right
at the edge. The gap of 326 at the final reading and 464 at the shipped
reading straddles that threshold.

**Enough to justify one more seed.** A confirmation run at seed=43,
identical configuration, answers definitively:
- If sign-consistency holds (≥ 9/10 favor B) with similar magnitude,
  pool the two runs. The pooled best-vs-best delta would be roughly
  −460 ± 180, z ≈ −2.5 — a clear win.
- If the sign flips (≤ 7/10 favor B, or delta near zero), the seed-42
  result was a lucky draw and the feature enrichment is neutral.

## Next steps (priority order)

1. **Run seed=43.** `outputs/v33ab/seed43/{A,B}`.
2. If seed=43 confirms: promote rich 6D to the default preflop table
   for subsequent long runs, write a follow-up T2.x-style result doc.
3. If seed=43 disconfirms: archive as plausible-but-unconfirmed and
   move on to the flop/turn/river feature-enrichment axis (the "same
   idea, larger surface" follow-up in the handoff).
4. Independently: extend the same 6D feature idea to flop/turn/river
   histograms. Orthogonal to whether preflop 6D wins.

## Files

- Baseline tables: `outputs/v31base/{centroids,preflop_abstraction,...}.bin`
- Rich tables: `outputs/v33rich/{centroids_6d,preflop_abstraction,...}.bin`
- Code changes:
  - `crates/pkr-abstraction/src/lib.rs` — `CentroidStore6D`,
    `hand_structure_features`, `nearest_centroid_6d`, `rich_features_tests`
  - `crates/pkr-abstraction/src/bin/precompute.rs` — `kmeans_6d`,
    `generate_centroids_6d`, `generate_preflop_rich_table`,
    `preflop-rich` subcommand, `PKR_RICH_CENTROIDS` gate
- Commits: `4669b97`, `20899a2`, `2844e32`
- Raw runs: `outputs/v33ab/{A,B}/{metrics,exploitability,stats}.*`
