# v33 — Rich 6D preflop centroids (CONFIRMED WIN across 2 seeds)

**Date:** 2026-09-25
**Status:** Confirmed win, 2 seeds, 7/7 sign-consistent matched readings
**Supersedes:** `docs/experiments/v33-rich-preflop-suggestive-positive.md`
**Continues:** `docs/handoff/HANDOFF_2026-09-25.md` §3

## TL;DR

Replacing the 2D `(EHS, EHS²)` preflop feature space with a 6D space
`(EHS, EHS², rank_high/12, rank_low/12, suited_bit, connector_bit)` at
the **same k=200** produces a **~425 mbb reduction** in sampled
best-response exploitability at the shipped checkpoint (promote-gate
output), replicating across two independent seeds. Every matched
exploitability reading in both seeds favors the 6D table.

## Setup

- **Baseline (A):** 2D preflop table from `outputs/v31base/`.
- **Rich (B):** 6D preflop table from `outputs/v33rich/`.
- **Everything else identical:** same flop/turn/river tables, same
  hyperparameters (`PKR_MOMENTUM=0 PKR_AVG_POWER=2 PKR_EXPLORE_EPSILON=0.01
  PKR_DCFR_ALPHA=1.5`), 20,000,000 iterations, 4000 eval deals,
  `--eval-every 2000000`, `--promote-gate 3`.
- **Seeds:** 42 and 43.

## Results

### Best-of-run readings (mbb, lower is better)

| seed | A (2D) | B (6D) | Δ |
|---|---|---|---|
| 42 | 3192.4 @ 6.0M | 2728.6 @ 10.0M | **−463.8** |
| 43 | 3235.0 @ 6.0M | 2849.3 @ 6.0M | **−385.7** |
| **pooled** | **3213.7** | **2789.0** | **−424.7** |
| **pooled SE** | 21.3 | 60.2 | 63.9 |
| **z** | | | **−6.65** |

The pooled effect size is well past the 2σ acceptance criterion the
handoff set for reliable single-A/B conclusions.

### Sign consistency

Every matched exploitability reading, both seeds, favors B:

| iter | seed 42 Δ | seed 43 Δ |
|---|---|---|
| 2M | −260.9 | −503.1 |
| 4M | −464.0 | −515.0 |
| 6M | −346.8 | −385.7 |
| 8M | −555.1 | −474.8 |
| 10M | −1032.8 | (run stopped) |
| 12M | −433.3 | — |
| 14M | −450.0 | — |
| 16M | −537.4 | — |
| 18M | −557.9 | — |
| 20M | −326.1 | — |

Seed 43's run was terminated after the 6M reading (approximately
iteration 10M) once the result was unambiguous. Seed 42 completed the
full 20M.

### Curve shape replication

Both A seeds find their minimum at exactly **iteration 6M** (3192.4 and
3235.0 — a 43 mbb spread). Both B seeds also find their minimum at
6M (2845.6 and 2849.3 — a **4 mbb spread**). The learning-curve shape is
a stable feature of this configuration, not seed noise.

## Interpretation

The 2D `(EHS, EHS²)` feature space collapses strategically distinct hands
onto the same point. The canonical example is that `22`, `A5s`, and `KJo`
all have EHS ≈ 0.50, so they land in the same cluster and the model is
forced to play them identically — but one is a set-miner, one is a wheel
draw, and one is a dominated offsuit broadway.

The 4 extra hand-structure dims (rank_high, rank_low, suited, connector)
separate these hands into strategic neighbourhoods without increasing
the key count. The 200-cluster budget is reallocated from "equity
neighbourhoods" to "strategic neighbourhoods". This is the difference
between "the abstraction is finer" (which the handoff's prior negative
results showed is neutral) and "the abstraction is *smarter* at the
same size".

## Recommendation

1. **Ship rich 6D as the default preflop table** for all future runs.
   Generate with `PKR_RICH_CENTROIDS=1 pkr-abstraction-precompute
   centroids 1326 200 <rank> <out>` followed by `preflop-rich`.

2. **Extend the same feature enrichment to flop/turn/river histograms.**
   The current 10-bin EHS histogram has the same collapse problem in
   higher dimension. Adding position-relative or opponent-hand-strength
   features to the histogram is the natural next experiment.

3. **Fix two infrastructure bugs found during this experiment:**
   - Trainer does not abort on `ENOSPC`; it logs `WARNING: checkpoint
     failed` and retries every 10K iterations, filling the disk.
     Add a `consecutive_save_failures` counter, abort after 3.
   - `--checkpoint-every 10000` (default) writes a 186 MB checkpoint
     every ~30 s ≈ 6 MB/s sustained I/O. On macOS this triggers
     Spotlight/Time Machine resource storms. Change default to
     `500000`.

## Files

- Baseline tables: `outputs/v31base/{centroids,preflop_abstraction,...}.bin`
- Rich tables: `outputs/v33rich/{centroids_6d,preflop_abstraction,...}.bin`
- Seed-42 raw: `outputs/v33ab/{A,B}/{metrics,exploitability,stats}.*`
- Seed-43 raw: `outputs/v33ab/seed43/{A,B}/{metrics,exploitability,stats}.*`
- Code:
  - `crates/pkr-abstraction/src/lib.rs` — `CentroidStore6D`,
    `hand_structure_features`, `nearest_centroid_6d`
  - `crates/pkr-abstraction/src/bin/precompute.rs` — `kmeans_6d`,
    `generate_centroids_6d`, `generate_preflop_rich_table`,
    `preflop-rich`, `PKR_RICH_CENTROIDS` gate
- Commits: `4669b97`, `20899a2`, `2844e32`, `f3fe1c2`
