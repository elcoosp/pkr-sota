> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

# v36 — capacity sweep + iters-per-sync sweep (FINAL)

**Date:** 2026-09-26
**Status:** Both sweeps complete.

## Capacity sweep (FINAL)

Same config as v33-B / v35-A except `--capacity`, 20M iterations, seed 43 (plus existing seed-42 anchors).

| seed | 5M best | 60M best | delta (60M − 5M) |
|---|---|---|---|
| 42 | 2728.6 | 2650.1 | -78.5 |
| 43 | 2638.5 | 2604.7 | -33.9 |
| **pooled** | **2683.6** | **2627.4** | **-56.2** |

Pooled SE ≈ 50.5 mbb → z = -1.11. Direction consistent across both seeds (−78.5 and −33.8), pooled effect -56.2 mbb.

**Verdict:** Within noise at 2 seeds, but sign-consistent. **Shipped 60M as the default** (`binaries/pkr-trainer/src/main.rs` 44d3432). The extra capacity costs virtual address space only (RSS measured ~700 MB stable at both capacities).

## Iters-per-sync sweep

3 fresh runs at seed 42, 60M capacity, 5M iterations, `--iters-per-sync ∈ {256, 1024, 2048}`. Reference: v35-A at 512 (20M iters, take the 5M reading).

| sync | best mbb @ 5M | throughput (it/s) |
|---|---|---|
| 256 | 2909.6 | 7,767 |
| 1024 | 2771.1 | 9,091 |
| 2048 | 2770.9 | **10,228** |

**Throughput:** 2048 is +13% vs 1024, +32% vs 256, +20% vs 512 (v35-A's ~8.5K it/s on the same config). Exploitability at 5M is noisy (2 readings each); the 1024 vs 2048 numbers are effectively identical (2771.1 vs 2770.9), so the throughput gain comes at no measured convergence cost up to 5M iterations.

**Caveat:** only 2 readings per config, 5M iterations. A longer comparison would strengthen the convergence claim. But 256's 2909 looks worse than 1024/2048's ~2771, which suggests lower sync values hurt convergence at this training scale (more staleness-per-batch actually seems fine — or the ordering is a coincidence).

**Recommendation:** ship `--iters-per-sync 2048`.

## Cross-seed variance at 20M iterations

Session data:
- seed 42 vs 43 at 5M capacity, 20M iters: 2728.6 vs 2638.5 → SD ≈ 90 mbb
- seed 42 vs 43 at 60M capacity, 20M iters: 2650.1 vs 2604.7 → SD ≈ 45 mbb

Smaller than expected. Handoff's "78 mbb at 5M iters" guidance remains conservative. Use **~100 mbb** for A/B decisions at 20M iters.

## Best 20M-iteration reading this session

**2604.7 mbb** (v36 cap60M seed 43). Below both seed-42 anchors.

## Artifacts

- CSVs archived to `outputs/archive/csvs/v36*__*.csv`
- Checkpoints cleaned from `outputs/v36*` to reclaim disk
- Commit `44d3432` — capacity default 60M
