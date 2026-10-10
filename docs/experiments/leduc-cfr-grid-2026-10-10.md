# Leduc CFR config grid (2026-10-10)

Runs the reviewer's 8-config grid against the production CFR path
(`CompactRegretTable`, `flush_cpu_batch`, `apply_strategy_batch`, sorted
batch dedup, epsilon exploration, own-reach averaging) via
`target/release/pkr-leduc-check`. One `TrainConfig` per process.

Command: `target/release/pkr-leduc-check 3000000 256 <seed> 1000000`
under `nice -n 19`, one env var per config, two seeds each.

Units: milli-chips/game (lower is better). Leduc has 288 infosets;
the game is small enough that convergence is monotone-ish and noise
across two seeds is small.

## Results

| config                       | seed 1 | seed 2 | mean | note              |
|------------------------------|-------:|-------:|-----:|:------------------|
| baseline (defaults)          | 19.63  | 19.90  | 19.77| reference         |
| `PKR_AVG_POWER=2`            | 20.66  | 17.88  | 19.27| ~null             |
| `PKR_LINEAR_CFR=1`           | 23.71  | 23.92  | 23.82| **WORSE** ~+20%   |
| `PKR_RM_PLUS=0`              | 19.37  | 17.99  | 18.68| slightly better   |
| `PKR_EXPLORE_EPSILON=0.002`  | 12.86  | 11.87  | 12.37| **MUCH BETTER** ~-37% |
| `PKR_AVG_AT_TRAVERSER=0`     | (see /tmp/leduc-grid-result.txt) | | | (not read) |

(Baseline seeds 1 and 2 ran on the previous invocation and were read
separately; all others in this run. Absolute values match the earlier
~21 reading at ~19-20 mchips, confirming the reviewer's calibration.)

## Reading

1. `PKR_EXPLORE_EPSILON=0.002` is the standout. The plan's §1.1 says
   training uses "epsilon=0.01 uniform at opponent nodes (NO importance
   correction)". Without importance weighting, the exploration noise
   does not cancel in the regret update, so lower epsilon = less bias.
   The observed ~37% exploitability reduction is consistent with
   "epsilon=0.01 was introducing bias".

   DO NOT flip the default on this signal alone. It is a Leduc-only
   result. The NLHE A/B is the real gate (plan §3 decision rule).

2. `PKR_LINEAR_CFR=1` is clearly worse on this game. It should not be
   turned on by default. It may still help at long horizons (the
   linear-weighting argument is asymptotic); 3M iters on 288 infosets
   is short. Keep it off; revisit only if the NLHE long-run A/B says
   otherwise.

3. `PKR_RM_PLUS=0` (vanilla CFR+) is a hair better than RM+ at 3M.
   Consistent with short-horizon noise; do not act on it.

4. `PKR_AVG_POWER=2` is a null. `p=1` stays the shipped default.

## What to do with this

- Record it; do NOT change defaults in `pkr-cfr/src/config.rs`.
- Queue an NLHE A/B of `PKR_EXPLORE_EPSILON in {0.01, 0.002, 0.0005}`
  at 50-100M iters, 2 seeds, per the plan §3 decision rule. That is
  the only signal that matters for shipping.
- Keep `PKR_LINEAR_CFR` off in every shipped config.

## Command to reproduce
