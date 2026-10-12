# HANDOFF - 2026-10-09 overnight run (compute queue)

Continues HANDOFF_2026-10-09_session6.md.

## Compute queue (14h window)

1. `/tmp/sweep32b.sh` -> `/tmp/sweep32-full.log`
   Task 3.2: iters-per-sync 256 vs 2048, 20M each, cap 20M, 4 threads.
   Ends with "SWEEP2 DONE". LBR (10k) per arm.
2. `/tmp/pruneab2.sh` -> `/tmp/pruneab-full.log` (waits for SWEEP2 DONE)
   Task 3.1 acceptance: unpruned vs PKR_PRUNE=1, 30M each, cap 20M.
   Ends "PRUNE2 DONE". LBR (20k) per arm.
   ACCEPT: pruned LBR within 1 SE of unpruned AND wall-clock down >=25%.
3. (3.3 long run was NOT re-queued; disk/mem too tight. Re-add if room.)

## Reading the results

  grep -E "SWEEP2 DONE|mbb_per_hand" /tmp/sweep32-full.log
  grep -E "PRUNE2 DONE|wall=|mbb_per_hand" /tmp/pruneab-full.log

## Machine hazards seen this session

- Foreign processes drove load to 100-160 on 8 cores; runs were 2-3x slow.
- MEMORY is the real limit: 3.4k free pages at times; the first 30M-capacity
  sweep died silently (likely OOM). Relaunched at capacity 20M (1.1 GB RSS).
- Disk is tight (~10-13 GB). Freed ~2.4 GB by deleting byte-identical
  blueprint.best.bin + finalized-run train.ckpt.prev files.
- Always LOG the full trainer output to a file; a bare "--fresh" run that
  dies gives no clue otherwise.

## Verified results already banked (docs/experiments/VERIFIED_FINDINGS.md)

- Vector search: raw BR 2.16 vs blueprint 8.44; bound H2H +0.82..+1.18
  (4 seeds, t>4); gadget H2H +0.62 (t=5.14) and +0.48 (t=3.78).
- Both gadgets PROVEN safe (EV cbv); bound PROVEN safe.
- V4 == V5 in self-play H2H; LBR gap 76% off-tree.
- 3.4 profile: traverse 75%, flush 20%, merge <1% -> no perf work.
- 3.1 pruning wired observable (1.5-1.9M pruned/window).
- 2.3 histogram features complete + 11 tests
  (river_equity/turn_hist/flop_hist/kmeans_cdf/nearest_centroid_cdf/
  mean_within_cluster_emd/cdf/emd_l1).

## Still open (need compute/network beyond this window)

- 2.3(d/e): scale buckets + measure RSS (needs a precompute run).
- 2.4: K=6->7 (checkpoint-invalidating).
- 5: depth-limited flop (needs blueprint-EV tables).
- 6: 200bb + Slumbot (network + sample_api.py).
