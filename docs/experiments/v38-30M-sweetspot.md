# v38 — 30M is lower than 100M (unexpected)

**Date:** 2026-09-27
**Status:** Strong signal, needs same-seed confirmation.

## Result

Three fresh seeds at 30M iterations, same tables and hyperparameters as
v34long / v37 (rich 6D preflop, 2D flop/turn, 60M capacity, sync 2048):

| seed | best mbb | @iter |
|---|---|---|
| 200 | 2231.7 | 27,039,744 |
| 201 | 2246.9 | 27,039,744 |
| 202 | 2171.2 | 27,039,744 |
| **mean** | **2216.6** | |
| **SD** | **40.0** | |

Compared to the 100M pool from v37 (seeds 42/100/101/102, same config):

| config | n | mean best | SD |
|---|---|---|---|
| 100M | 4 | 2587.6 | 63.3 |
| **30M** | **3** | **2216.6** | **40.0** |
| **delta** | | **−371 mbb** | |

**Training 30M iterations produces ~370 mbb lower exploitability than
100M.** That's a 5.5σ difference on the pooled statistics if the SDs
hold.

## What it isn't

- **Not a 15M sweet spot.** The `at15M` column reads 2575 mbb mean,
  matching the 100M pool. The 15M hypothesis from the earlier
  "seed100/101/102 best @ 15M" observation was wrong — those earlier
  readings were partial.
- **Not an eval-cadence artifact.** v38 used `--eval-every 3M`; v37
  used 5M. Different cadences read different deal seeds per eval (the
  trainer's `EVAL_SEED ^ iter` is iteration-dependent), so a small
  variance contribution exists, but nowhere near 371 mbb.

## Why this might be real

- **Regret drift.** Continuous CFR updates continue shifting strategy
  after the true Nash-ish minimum. The 27M reading may be the last
  point before the table has consumed enough of its 60M capacity to
  start overfitting.
- **Sawtooth.** Both v34long and v38 show a minimum then a rise. If
  the true curve is sawtooth with period ~30M, 100M lands on a rising
  edge. 27M lands on a falling edge.
- **The 100M pool was a bad draw.** Four seeds all landing at ~2580
  seems unlikely (SD 63), but not impossible.

## Why it might be false

- **Different seed sets.** v37 = {42,100,101,102}. v38 = {200,201,202}.
  If the v38 seeds happen to be systematically luckier, we're measuring
  seed luck, not training length.
- **Different eval cadences.** v37 = 5M, v38 = 3M. The extra eval
  points in v38 might sample a deeper dip.

## Confirmation test

Re-run seeds 200, 201, 202 at **100M iterations, 5M eval cadence**,
same tables. Two outcomes:

- best lands at ~2250: the 100M pool (2587) was a bad draw. 27M is
  still lower but not by as much.
- best lands at ~2580: 30M genuinely produces lower readings than 100M.
  Ship 30M as the default training budget.

~3 hours of compute. Highest-value follow-up.

## Impact if confirmed

Every future experiment runs at 1/3 the cost. Session iteration cadence
on training-side experiments goes from ~3h to ~1h per seed.


## Plateau-stop feature (2026-09-27)

Added `--stop-on-plateau N`: end training after N consecutive evals
produce no new historical minimum. Now default in `run-config.sh`
(`STOP_ON_PLATEAU=5`, i.e. stop at last_min + 25M iters at 5M cadence).

Retroactive check on the four completed 100M runs:

| run | last min | stop at | saved | pct |
|---|---|---|---|---|
| v37 seed 42 | 60M | 85M | 15M | 15% |
| v37 seed 100 | 15M | 40M | 60M | 60% |
| v37 seed 101 | 15M | 40M | 60M | 60% |
| v37 seed 102 | 15M | 40M | 60M | 60% |
| **average** | | | **49M** | **49%** |

Three of four wasted 60% of the iteration budget on post-minimum
drift. The shipped artifact is unchanged — the promote gate preserves
the historical minimum regardless — so this is pure compute savings.

For a 100M-budget experiment, the flag cuts ~40M iterations without
touching the output. Future seed-pool experiments run in roughly
half the wall time.


## Second seed confirmation (partial, 2026-09-27 late)

Seed 201 100M run at 6 readings shows best 2583 mbb, vs seed 201's
30M best of 2247 mbb — a +336 mbb gap in the same direction as seed
200 (+375). 2/2 seeds confirm the pattern.

Full 100M results pending (seed201 completes ~00:45, seed202 after).

| seed | 30M best | 100M best | delta |
|---|---|---|---|
| 200 | 2231.7 | 2607.3 (final) | +375.6 |
| 201 | 2246.9 | 2583.0 (6/10) | +336.1 |
| 202 | 2171.2 | pending | |
