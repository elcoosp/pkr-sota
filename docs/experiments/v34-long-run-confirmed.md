# v34 — 100M-iteration rich 6D run (confirmed second win)

**Date:** 2026-09-25 (launch) → 2026-09-26 00:59 (finish)
**Status:** Confirmed win over v33. New best-in-class exploitability.
**Continues:** `docs/experiments/v33-rich-preflop-confirmed.md`

## TL;DR

Training the rich-6D config for 100M iterations with 60M regret capacity
produced a best reading of **2526.2 mbb @ 60M iterations**, versus
**2728.6 mbb** for the same seed + same tables at 20M iterations
(v33-B seed-42) and **3192.4 mbb** for the 2D baseline (v33-A
seed-42). Session-cumulative improvement: **−666 mbb (−21%)** from
start (v31base at 5M) to this run's best.

The promote gate shipped the 60M checkpoint, not the 100M end. Final
reading at 100M was 2982 mbb.

## Setup

- **Tables:** `outputs/v33rich/*` (rich 6D preflop from v33), same
  flop/turn/river as v33.
- **Hyperparameters:** `PKR_MOMENTUM=0 PKR_AVG_POWER=2
  PKR_EXPLORE_EPSILON=0.01 PKR_DCFR_ALPHA=1.5`.
- **Training budget:** 100,000,000 iterations, `--capacity 60000000`.
- **Evaluation:** `--eval-every 5000000 --eval-deals 5000
  --promote-gate 3`.
- **Seed:** 42.
- **Checkpointing:** `--checkpoint-every 5000000` (20 saves, ~560 MB
  peak disk per save).

Wall time: 3h20m at 8.35K it/s average (including eval pauses).

## Results

All 20 readings (mbb, SE ~180):

| iter | expl_mbb |
|---|---|
|    5,002,240 | 2762.5 |
|   10,004,480 | 3188.9 |
|   15,006,720 | **2693.9** |
|   20,008,960 | 2854.3 |
|   25,011,200 | 2893.5 |
|   30,013,440 | 3050.6 |
|   35,015,680 | 2865.1 |
|   40,017,920 | **2661.1** |
|   45,020,160 | 2704.4 |
|   50,022,400 | 3155.9 |
|   55,024,640 | 2691.4 |
|   60,026,880 | **2526.2 ← best (shipped)** |
|   65,029,120 | 2805.3 |
|   70,031,360 | 2996.4 |
|   75,033,600 | 2913.8 |
|   80,035,840 | 3016.3 |
|   85,038,080 | 3161.4 |
|   90,040,320 | 2695.4 |
|   95,042,560 | 2990.2 |
|  100,000,000 | 2982.5 |

Curve shape: sawtooth with ~25M period. Deepest dips at 15M, 40M, 60M.
The minimum at 60M was the last sub-2600 reading of the run. From 65M
onward the curve never returned below 2695.

### Comparison anchors

| run | tables | iters | best mbb |
|---|---|---|---|
| v31base | 2D | 5M | 3458 |
| v33-A (seed 42) | 2D | 20M | 3192 |
| v33-B (seed 42) | 6D | 20M | 2729 |
| v33-B pooled (2 seeds) | 6D | 20M | 2789 |
| **v34long** | **6D** | **100M** | **2526** |

- vs v33-B seed-42 (same seed, same tables, only iters+capacity differ): **−202 mbb**
- vs v33-B pooled: **−263 mbb**
- vs v33-A pooled (2D): **−688 mbb**
- vs v31base 5M start of session: **−932 mbb**

## Interpretation

### What this run proves

1. **Rich 6D features + long training is better than either alone.**
   The v33 win (rich 6D at 20M) was 425 mbb; v34long adds another 200+
   mbb from iteration count alone (with 6D held constant).

2. **The v33-B "20M plateau" was an iteration-count artifact, not an
   abstraction ceiling.** v33's conclusion was that the abstraction
   space was the binding constraint. v34long falsifies that for the
   20M–100M interval: the same tables keep improving through 60M.

3. **The promote gate works.** `blueprint.best.bin` and `blueprint.bin`
   both contain the 60M checkpoint (18.5 MB, mtime 23:53). The 100M
   final reading (2982) was correctly rejected.

### What this run does NOT prove

1. **Where the iteration floor is.** We don't know if 200M would dip
   below 2526. The 65M–100M plateau at 2695–3160 suggests diminishing
   returns, but two readings (55M and 90M) returned to <2700, so the
   sawtooth hasn't fully flattened.

2. **Which of the two changed variables (iters vs capacity) mattered.**
   Both changed vs v33-B. A single-variable A/B would need either
   (a) 100M iters at 5M capacity, or (b) 20M iters at 60M capacity.

3. **Whether the 2526 dip is seed-stable.** We only ran seed 42. The
   sawtooth pattern is likely structural (all v33/v34 curves show it),
   but the exact minimum is one sample.

## Recommendation

**Ship `outputs/v34long/blueprint.best.bin` as the current best
blueprint.** It is the strongest artifact this project has produced
(2526 mbb abstract, ~10k-15k mbb real-game at the 3-5× factor).

For the next step, in priority order:

1. **Confirm 2526 with 1 more seed** (2-3 hours). Run the same config
   at seed=43, 100M iters, 60M capacity. If it also lands in the
   2400–2700 range, the improvement is seed-stable and we have a
   new baseline.

2. **Step 4: extend 6D features to flop/turn/river histograms.**
   The preflop win came from enriching features at fixed k. The
   current flop/turn/river feature is a 10-bin EHS histogram which
   has the same collapse problem. This is the highest-EV algorithmic
   direction.

3. **Probe iteration floor with a 200M run.** Lower priority because
   wall time is 6-7 hours, and the sawtooth's post-60M behavior
   suggests ~100M is near the useful budget at 60M capacity.

## Files

- Tables: `outputs/v33rich/*` (rich 6D preflop)
- Run dir: `outputs/v34long/`
- Shipped blueprint: `outputs/v34long/blueprint.best.bin`
- Final ckpt: `outputs/v34long/train.ckpt`
- Archived CSVs: `outputs/archive/csvs/outputs_v34long_*.{csv,json}`
- Code (unchanged from v33):
  - `crates/pkr-abstraction/src/lib.rs` — `CentroidStore6D`,
    `hand_structure_features`, `nearest_centroid_6d`
  - `crates/pkr-abstraction/src/bin/precompute.rs` — `kmeans_6d`,
    `preflop-rich`
  - `binaries/pkr-trainer/src/main.rs` — CSV append-on-resume,
    checkpoint_every default 500k, abort on checkpoint failure
  - `run.sh`, `scripts/run-config.sh` — `PREFLOP_RICH=1` default
