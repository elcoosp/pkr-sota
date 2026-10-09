# Task 3.1 pruning - COMPLETE + measurable (2026-10-09)

The prune path existed but `pruned_actions` was collected in
LocalMetrics and never merged/recorded/snapshotted/written - so it was
unobservable. Now wired end-to-end:

  LocalMetrics.pruned_actions -> merge_from -> GlobalMetrics.pruned_actions
  -> record_batch -> Snapshot -> delta -> trainer metrics.csv column
  -> ci/scripts/parse-metrics-csv.py benchmark.

Verified: a smoke run with PKR_PRUNE=1 PKR_PRUNE_WARMUP=10000 shows
pruned_actions = 1,552,459 / 1,810,062 / 1,964,944 over 3 windows
(i.e. pruning IS active), and the CSV header has the column, and the
parser emits proftest/pruned_actions.

## Still to measure (needs compute)

The plan acceptance: at equal iters (50M), LBR within 1 SE of unpruned
AND wall-clock down >= 25%. That is a real training A/B - not run here.

## 3.2 sweep status

Running (256 vs 2048 iters-per-sync, 20M iters each) but VERY slow:
foreign processes have the machine at load ~100-160 on 8 cores, so the
trainer gets ~2800 it/s instead of ~7000. Result pending.
