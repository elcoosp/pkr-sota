> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

# Seed variance of the sampled-BR metric

**Measured 2026-09-25 on v31base tables, current code (α=1.5, k=200).**

Five independent runs, same config, only `--seed` varies (1..5). Each
run: 5M training iters, one eval at 4000 deals.

| seed | expl_mbb | eval SE | wall |
|------|----------|---------|------|
| 1    | 3702     | ±195    | 16m  |
| 2    | 3792     | ±199    | 10m  |
| 3    | 3744     | ±203    | 23m  |
| 4    | 3906     | ±202    | 9m   |
| 5    | 3751     | ±196    | 8m   |

- **mean**    = 3779 mbb
- **sample SD** = 78 mbb
- **range**   = [3702, 3906]   (spread 204)
- **avg eval SE** = 199 mbb
- **ratio (seed SD / eval SE) = 0.39**

## Interpretation

**Single-seed A/Bs are reliable.** Seed-to-seed variance (~78 mbb) is
less than half the SE of any one eval (~200 mbb). A difference between
two runs of >400 mbb (2σ combined) is a real effect, not noise.

**Earlier conclusions hold.** All the overnight A/Bs (2000 deals, ±300
SE) drew "neutral" conclusions from differences <300 mbb. Those calls
were correct.

**The 3458-vs-5511 mystery is fully explained.** v27a15 (2000 deals,
5511) translates to ~3940 at 4000-deal scale using the deal-count ratio
we measured; that's within 1σ of the seed-sweep mean (3779). No hidden
bug, no cross-binary difference.

## Consequences

- Use `--eval-deals 4000` for all future A/Bs (canonical setting).
- Single-seed runs are sufficient for A/B decisions at >400 mbb delta.
- For marginal differences (100-400 mbb), average 2-3 seeds.
