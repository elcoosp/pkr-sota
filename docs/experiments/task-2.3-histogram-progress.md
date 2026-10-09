# Task 2.3 - distribution-aware features: progress (2026-10-09)

## DONE (committed, tested)

`crates/pkr-abstraction/src/histogram.rs`:
- 2.3(b) `river_equity` - EXACT equity vs a uniform random hand on a
  complete 5-card board. Tested against brute force + nuts/worst.
- 2.3(b) `turn_hist` - histogram of the 46 river equities (exact).
  Tested: sums to 1, deterministic.
- 2.3(b) `flop_hist` - histogram over 47 turns, each a sampled river
  estimate (samples_per_turn). Tested: sums to 1, deterministic.
- 2.3(c) `cdf` + `emd_l1` - CDF and earth-movers (L1-of-CDF) distance
  for clustering histograms. Tested (identity=0, symmetry, monotone).

7 histogram tests pass; full pkr-abstraction lib = 47 pass.

## NOT done

- 2.3(a) suit-iso: DECLINED (measured 1.21x dedup).
- 2.3(d) bucket-count scaling + RSS measurement.
- 2.3(e) river feature redesign (river is already rank-exact).
- The ACTUAL PRECOMPUTE + retrain + LBR A/B that would USE these
  features. That is multi-hour compute AND the machine is currently
  at load 160/8 (foreign), so it cannot run now.

## What these features enable (next session)

Wire `turn_hist`/`flop_hist` into precompute.rs, cluster with k-means
on CDFs (emd_l1), then train + LBR A/B vs the (EHS,EHS^2) buckets.
The functions are ready and tested; only the run is missing.
