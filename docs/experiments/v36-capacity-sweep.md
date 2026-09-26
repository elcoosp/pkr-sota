# v36 — capacity sweep (FINAL)

**Date:** 2026-09-26
**Status:** 60M capacity beats 5M by **-56.2 mbb** pooled across 2 seeds. Direction is consistent, magnitude within 1 sigma of cross-seed noise but positive on both seeds.

## Result

Same configuration as v33-B / v35-A except capacity. 20M iterations, 5000-deal evals.

| seed | 5M best | 60M best | delta (60M - 5M) |
|---|---|---|---|
| 42 | 2728.6 (v33-B) | 2650.1 (v35-A) | -78.5 |
| 43 | 2638.5 (v36) | 2604.7 (v36) | -33.9 |
| **pooled** | **2683.6** | **2627.4** | **-56.2** |

Both seeds favor 60M. The magnitude is not statistically distinguishable from zero at 2 seeds (SD ~110 mbb), but the sign consistency (2/2) suggests a real if modest effect.

## Interpretation

Regret tables at 5M capacity hit collisions earlier in the run; 60M defers them. At 20M iterations, the 5M table is at ~24% utilization (based on v34long's trajectory) vs 60M at 2%. The extra headroom means fewer regrets compete for the same slot during early training.

The effect is small (-56.2 mbb at 20M iters) but costs nothing — 60M capacity's additional memory footprint is lazily allocated virtual address space, not RSS (measured ~700 MB stable across both caps).

## Recommendation

**Change the default `--capacity` from 5_000_000 to 60_000_000.**

- `run-config.sh` line 36: `CAPACITY="${CAPACITY:-60000000}"`
- `binaries/pkr-trainer/src/main.rs` line 88: `#[arg(long, default_value_t = 60_000_000)]`
- Update the handoff's "production flags" to include `--capacity 60000000`

## Cross-seed variance observation

Handoff §2 documented SD = 78 mbb across 5 seeds at 5M iterations. At 20M iterations, cross-seed spread is ~90-130 mbb on matched configs. SD grows with iteration count.

## Related

- `docs/experiments/v34-long-run-confirmed.md` — same config at 100M iters, best 2526
- `docs/experiments/v35-flop-turn-rich-negative.md` — negative result on rich flop/turn features
- `docs/handoff/HANDOFF_2026-09-25.md` §2 — historical context
