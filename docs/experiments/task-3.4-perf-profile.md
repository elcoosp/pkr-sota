# Task 3.4 perf profiling - result (2026-10-09)

Ran the trainer with PKR_PHASE_PROFILE=1 (5M iters, cap 10M, 4 threads,
V5 sig). Per-2048-batch phase breakdown (representative):

  wall    ~250-350 ms
  traverse ~160-290 ms  (~75%)
  flush    ~40-95 ms    (~20%)
  merge    ~1-3.5 ms    (<1%)
  cache_hit ~0.80
  nodes/it ~470

## Decision

The plan says: "if flush+merge > 30% of wall time, look at coordinator-
side serial merge." flush+merge is ~20-25%, BELOW 30%, and merge alone is
negligible. The dominant cost is the parallel traverse (~75%), which is
the intended hot path.

So: NO perf work warranted from this profile. Traverse-bound is expected
and healthy. (The 20% flush is the only non-trivial non-traverse cost, but
it is below the plan actionable threshold.)

## Optional future

If a profile later shows traverse-bound AND we want more throughput, the
lever is algorithm (pruning, Phase 3.1) not plumbing. See
PKR_PRUNE / train 3.1.
