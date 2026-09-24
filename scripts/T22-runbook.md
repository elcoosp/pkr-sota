# T2.2 result runbook

## What T2.2 changed

- river hand-tier shift: >> 15 (287 tiers)  ->  >> 13 (1152 tiers)
- RIVER_BUCKETS: 200 -> 128
- Fingerprint now encodes river_tier_shift (a pre-T2.2 checkpoint
  loaded against a post-T2.2 binary is rejected, not silently used).

## The run

- Version: v26a
- 200M iters, 6 threads, evals at every 20M iters
- Flags: PKR_MOMENTUM=0 PKR_AVG_POWER=2 PKR_EXPLORE_EPSILON=0.01
- Same 10-dim EHS histograms for board buckets; only hand tiering changed

## What to compare against

v25final (pre-T2.2, same flags):

  iter     expl_mbb    sigma
   20M      5735      317
   40M      5812      350
   60M      6031      328   (peak)
   80M      5581      338
  100M      5484      353
  120M      5450      359   (lowest)
  140M      5687      351

v25final was flat at 5400-5700 after 80M. If T2.2 helps, v26a
should be lower at matched iterations, especially post-80M.

## How to read it

Use scripts/compare-runs.py:

  python3 scripts/compare-runs.py \
    outputs/v25final/exploitability.csv \
    outputs/v26a/exploitability.csv

It prints delta and significance per matched iteration.

Decision rules:

| Observation                         | Interpretation            | Next        |
|-------------------------------------|---------------------------|-------------|
| v26a 20M < 5400 (-2 sigma vs v25f)  | T2.2 is a big lever       | extend v26a to 400M |
| v26a 20M within 1 sigma of 5735     | T2.2 neutral at 20M       | wait for 100M+ |
| v26a 20M > 6300 (+2 sigma)          | T2.2 hurt early convergence | investigate (or revert) |
| v26a 100M+ < v25final - 2 sigma     | T2.2 wins late            | keep; T2.2 confirmed |
| v26a flat at ~5500                  | T2.2 neutral              | turn to eps A/B |

## Caveats

1. Only 6 threads (not 8) — the other agent is using 2 cores.
   Wall-clock per iteration will be ~30% longer than v25final.
   This does NOT affect expl_mbb at a given iteration count.

2. EHS_SAMPLES=100 for the river regen (was 1000 in v25final).
   River board buckets may differ slightly from a 1000-sample run.
   That is a controlled variable — the >> 13 hand tiering is what
   we are testing.

3. v26a's RIVER_BUCKETS=128 vs v25final's 200. Combined with the
   finer hand tiers, total river keyspace is ~2.5x v25final.
   More infosets = fewer visits each at fixed iteration count.
   Convergence may take more iterations to reach a stable low.

## Kill switch

  kill -INT $(cat /tmp/v26a.pid)

Saves a final checkpoint before exiting.
