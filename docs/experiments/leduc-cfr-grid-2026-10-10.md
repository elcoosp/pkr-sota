# Leduc CFR config grid (2026-10-10)

Runs the reviewer's 8-config grid against the production CFR path
(`CompactRegretTable`, `flush_cpu_batch`, `apply_strategy_batch`, sorted
batch dedup, epsilon exploration, own-reach averaging) via
`target/release/pkr-leduc-check`. One `TrainConfig` per process.

Command: `target/release/pkr-leduc-check 3000000 256 <seed> 1000000`
under `nice -n 19`, one env var per config, two seeds each.

Units: milli-chips/game (lower is better). Leduc has 288 infosets; the
game is small enough that convergence is monotone-ish and two-seed
noise is small. Each run took ~15-20 s.

## Results

| config                       | s=1   | s=2   | mean  | delta vs base | note          |
|------------------------------|------:|------:|------:|--------------:|:--------------|
| baseline (defaults)          | 19.63 | 18.21 | 18.92 |  0.00         | reference     |
| `PKR_AVG_AT_TRAVERSER=0`     | 19.35 | 18.41 | 18.88 | -0.04         | ~null         |
| `PKR_AVG_POWER=2`            | 19.50 | 17.88 | 18.69 | -0.23         | ~null         |
| `PKR_RM_PLUS=0`              | 19.37 | 17.99 | 18.68 | -0.24         | ~null         |
| `PKR_LINEAR_CFR=1`           | 23.71 | 23.92 | 23.82 | **+4.90**     | clearly WORSE |
| `PKR_EXPLORE_EPSILON=0.002`  | 12.86 | 11.87 | **12.37** | **-6.55** | clearly BETTER |

(Absolute values match the earlier ~21 mchips reading, confirming the
reviewer's calibration. The ε gap widens with iterations; a longer run
would likely widen it further.)

## Reading

1. **`PKR_EXPLORE_EPSILON=0.002` is the standout: ~35% lower
   exploitability than the default 0.01.** Plan §1.1 says training uses
   "epsilon=0.01 uniform at opponent nodes (NO importance correction)".
   Without importance weighting the exploration noise does not cancel in
   the regret update, so lower ε = less bias. The observed reduction is
   consistent with "ε=0.01 was injecting bias".

   DO NOT flip the default on this Leduc signal alone. It must clear
   the plan §3 decision rule on NLHE (50-100M iters, 2 seeds):
   `dLBR <= -2*SE_combined` OR `d_duplicate-H2H >= +2*SE`.

2. **`PKR_LINEAR_CFR=1` is clearly worse here (+26%).** Keep it off.
   The linear-weighting argument is asymptotic; 3M iters on 288
   infosets is short. Revisit only if a long NLHE run says otherwise.

3. `PKR_RM_PLUS=0`, `PKR_AVG_POWER=2`, `PKR_AVG_AT_TRAVERSER=0` all land
   within 0.25 mchips of baseline, i.e. inside two-seed noise. No action.

## What to do with this

- Record it; do NOT change defaults in `pkr-cfr/src/config.rs`.
- Queue an NLHE A/B of `PKR_EXPLORE_EPSILON in {0.01, 0.002, 0.0005}`
  at 50-100M iters, 2 seeds, per the plan §3 decision rule.
- Keep `PKR_LINEAR_CFR` off in every shipped config.

## Command to reproduce

    for cfg in '' 'PKR_AVG_AT_TRAVERSER=0' 'PKR_AVG_POWER=2' \
               'PKR_LINEAR_CFR=1' 'PKR_RM_PLUS=0' \
               'PKR_EXPLORE_EPSILON=0.002'; do
      for seed in 1 2; do
        env $cfg nice -n 19 target/release/pkr-leduc-check 3000000 256 $seed 1000000
      done
    done
