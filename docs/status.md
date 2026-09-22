# pkr-sota: Current Status

**Last updated:** 2026-09-22

This document is the source of truth for what is actually implemented and
working. For architectural rationale see `arch-overview.md`. For the
roadmap see `pkr-sota-winning-roadmap.md`.

---

## Pipeline status

Verified working end-to-end, on demand:

| Step | Command | Verified by |
|---|---|---|
| Precompute hand ranks | `pkr-abstraction-precompute hand_ranks` | `smoke.sh`, `proftest.sh` |
| Precompute centroids | `pkr-abstraction-precompute centroids` | `smoke.sh`, `proftest.sh` |
| Precompute per-street tables | `pkr-abstraction-precompute preflop / abs5 / turn / river` | `smoke.sh`, `proftest.sh` |
| Train CFR blueprint | `pkr-trainer` | `smoke.sh`, `proftest.sh` |
| Export blueprint | `pkr-export::write_blueprint` | `smoke.sh`, `proftest.sh` |
| Load + query at runtime | `pkr-runtime::SolverHandle` | `pkr-trainer::pipeline`, `pkr-runtime::roundtrip` |

98 tests pass across 11 binaries. 1 test ignored (`load_external_blueprint`,
which requires `PKR_BLUEPRINT` env var).

---

## Measured performance

From `proftest.sh` (8 threads, 50M capacity, k=64 centroids, flop table
loaded):

```
iter  5120/100000 | infosets: 110091 (0.2%) | 15196.1 it/s | cache_hit=0.584
iter 51200/100000 | infosets: 374078 (0.7%) | 21159.3 it/s | cache_hit=0.873
iter 100000/100000| infosets: 472881 (0.9%) | 27772.9 it/s | cache_hit=0.919
```

Steady state throughput: **~27,700 it/s**, ~2.4 billion iterations per day.
Training 100K iterations takes 3.6 s (5 s wall including init + export).

The first ~5000 iterations are slower because the thread-local idx cache is
cold (58% hit rate); by iteration 100K it warms to 92%.

Per-window metrics from the same run:
- **nodes/iteration**: 255–306 (first window slightly higher due to deeper
  exploration before regret-matching collapses the tree)
- **average tree depth**: 8.1–8.2
- **max |regret|**: 1.07e4 (bounded; no blow-up)
- **nonfinite regrets**: 0 (canonical discount is numerically safe)
- **regret op dedup ratio**: ~0.47 (half the pushed updates collapse to the
  same (idx, action) key in a 256-iteration batch, as expected)

### Extrapolated wall time

| Iterations | Wall time (steady state) |
|---|---|
| 10⁶ | ~40 s |
| 10⁷ | ~6 min |
| 10⁸ | ~1 hr (before infoset growth penalty) |
| 10⁹ | ~10 hr (with growth penalty; capacity issues likely) |

At 10⁹ iterations you hit the 50M default capacity; `--capacity` must be
raised to 200M+, which uses ~20 GB and does not fit on the target M1.

---

## Parallel training

Training uses rayon with `ITERS_PER_SYNC = 256`. Each dispatch runs 256
logical iterations split into chunks of 16. Each chunk:

1. Runs 16 traversal pairs (hero + villain perspective) into local buffers
2. Returns `(batch, strategy_batch, local_metrics)` to the coordinator
3. Coordinator merges all chunks, applies strategy ops and regret deltas in
   parallel via sort-dedup + `par_chunks`

This gives the first positive parallel scaling in the project's history:

| Threads | it/s | speedup |
|---|---|---|
| 1 | 15,635 | 1.00x |
| 2 | 26,879 | 1.72x |
| 4 | 33,734 | 2.16x |
| 8 | 38,657 | 2.47x |

At 8 threads the flush phase (sort-dedup + parallel apply) is ~30% of wall
time. Further scaling would need a redesign of the flush path
(per-thread regret tables merged every K iterations). This is not currently
a bottleneck: **even single-threaded throughput is 200x more than needed**
for the roadmap's "arena-playable" target.

Earlier attempts at parallel training were slower than single-threaded. The
fix was architectural: batch many logical iterations per dispatch rather
than syncing once per iteration. Per-iteration overhead was growing with
thread count because each iteration produced `num_threads` times as much
work to merge.

---

## DCFR status

The DCFR implementation was investigated with a Kuhn poker harness
(`crates/pkr-testgames`). Findings:

1. **`DiscountMode::RatioPower` was removed from the codebase.** The old
   formula `(t/τ)^p` multiplies regret by an unbounded factor on every
   update. For `t = 10⁶` that factor is ~3e4 per update; a dozen updates
   overflow f32. The Kuhn harness proved NaN at t≈3000, with `max|regret|`
   = 3.6e20 just before. **Every training run longer than ~3000 iterations
   was silently corrupting itself.** Production now uses only
   `DiscountMode::CanonicalDcfr`.

2. **Canonical DCFR is effectively vanilla CFR in f32.** The canonical
   factor is `t^p / (t^p + 1)`. For `t > 10⁴` with p=1.5, `t^p > 8.4e6`,
   so `+ 1.0` is below f32's epsilon and the factor rounds to exactly 1.0.
   Over a 10⁷-iteration run the discount only applies in the
   1000–10000 window where its effect is below the regret-ratio noise
   floor. **The docs previously claimed 2–10× faster convergence from
   DCFR. In this implementation it does not apply.**

3. **PCFR+ momentum has a small positive effect in the transitional range**
   and no measurable effect afterward. Kept on; not currently a tuning
   target.

4. **The `strategy_sum_discount_factor` (γ=2) function is not called from
   anywhere.** The exported average strategy is a plain unweighted
   sum. With γ=2 the cumulative discount over a full run is ~0.1% — below
   f32 precision. The function is dead code.

The Kuhn harness itself (`pkr-testgames`) has a correct CFR traverse and
exploitability computation via brute-force 2⁶ pure-strategy enumeration.
Exploitability converges from 0.25 to 1.24e-3 by t=3M for both vanilla and
canonical. Momentum-off converges ~3x faster than momentum-on on this game
— but that result does **not** transfer to NLHE (different scale, tree
shape, and sampling regime).

---

## Known limitations

### Crate-level

- `pkr-cfr::gpu` — The `GpuState` type is lazily constructed and never
  actually used in production (`flush_cpu_batch` is the production path).
  The GPU path exists only for a parity test. It works.
- `pkr-cfr::riversolve` — Not real CFR (regrets reset each iteration). Not
  called by the trainer. Retained for future depth-limited solve work.
- `pkr-cfr::valuenet` — Not wired into anything. Its
  `generate_training_data` returns synthetic labels. Retained for future
  depth-limited solving.
- `pkr-cfr::preflop_validate` — Validation helpers exist but are only
  exercised with dummy lookups in tests. Would be useful after the first
  real training run.
- `pkr-exploit` — Crate is defined but not integrated into training or
  serving. Implements opponent stat tracking and bounded exploit shifts.
- `pkr-fuzz` — Crate is defined but not integrated. Implements rules
  fuzzing and an eval harness against scripted bots.
- `pkr-export::fmph` — Builds an FMph structure but the runtime uses binary
  search. Either wire FMph into `SolverHandle` or drop it from the writer.

### Training-time

- **Capacity cliff**: at 50M infosets the trainer stops cleanly via
  `is_near_capacity`. At `--capacity` the `alloc_idx` panics loudly rather
  than silently clumping. 50M is the current default; 10⁸ iterations of
  real HU NLHE will exceed it.
- **Working-set growth**: past ~10M infosets, throughput drops as the
  papaya map and regret arrays stop fitting in cache.
- **No automated convergence measurement**: `stats.json` reports strategy
  distribution (pure/mixed/empty) and entropy histogram, but there is no
  in-process exploitability estimate for NLHE. The Kuhn harness measures
  exploitability for Kuhn only.

### Behavioral

- The bot has never been played. `proftest.sh` verifies the pipeline
  produces a blueprint with plausible structure (infosets registered,
  strategies non-uniform, no non-finite regrets), but nothing has been
  played against a scripted opponent or evaluated for actual poker quality.

---

## How to reproduce

```bash
# Fast: verify pipeline end-to-end
./smoke.sh

# Medium: production-scale training + metrics
./proftest.sh

# Real: full training run
#   1. Edit run.sh: ITERATIONS=10000000
#   2. ./run.sh
```

The `metrics.csv` and `stats.json` outputs from `proftest.sh` are the
handoff artifacts for external analysis. `blueprint.bin` is the trained
artifact itself.

---

## Changelog since initial commit

**Major fixes and refactors:**
- Removed `DiscountMode::RatioPower` (NaN at t≈3000); production uses only
  canonical now, and the mode enum no longer allows selecting a broken
  formula.
- Batched regret + strategy updates (sort-dedup + parallel apply) to fix
  parallel scaling.
- Batched logical iterations per rayon dispatch (`ITERS_PER_SYNC = 256`)
  to amortize serial merge.
- `strategy_sum` widened to i64 (i32 overflow at ~2.1M weighted visits).
- `legal_actions_into` — non-allocating variant used by the traversal.
- River infoset bucketing (`hand_rank >> 6`) to bound infoset count.
- Compact history signature (actions_this_street, num_raises, last_was_bet)
  replaces raw action bytes in the infoset hash.
- `MmapReader` + `SolverHandle` layout reconciled with
  `write_blueprint`'s output format.
- Checkpoint format v3 (i64 strategy_sum).
- `warn_nonfinite_regret_once` in `flush_cpu_batch` — catches future regret
  blow-ups in real time.

**Added instrumentation:**
- `pkr-cfr::metrics` module with `LocalMetrics`, `GlobalMetrics`,
  `Snapshot`.
- `pkr-trainer --metrics-csv` writes per-window CSV rows.
- `pkr-trainer --stats-json` writes end-of-run strategy analysis + sampled
  infosets.
- `PKR_PHASE_PROFILE=1` env var emits per-batch phase timings.
- `pkr-testgames` Kuhn harness.

**Infrastructure:**
- `smoke.sh` — end-to-end pipeline verification.
- `proftest.sh` — production-scale profile run with metrics.
- `bench.sh` — thread-scaling benchmark (superseded by proftest).

---

## What's next (in priority order)

1. **Play the bot.** Load `blueprint.bin` into the host app, run 100 hands
   against scripted bots (see `pkr-fuzz::run_eval_harness`), verify no
   visibly insane decisions.
2. **Scale training.** If the play test looks sane at 100K iterations,
   run `./run.sh` with `ITERATIONS=10000000`.
3. **Wire `pkr-fuzz` into the trainer.** Automated eval against
   Station/Nit/Aggro bots to catch quality regressions.
4. **Wire `pkr-exploit`.** Bounded opponent-shift overlay; the roadmap's
   P0 lever against weak tournament fields.
5. **Consider dropping `pkr-cfr::gpu`, `riversolve`, `valuenet`,
   `preflop_validate`** if they are not going to be wired in.

Everything else is optimization past the point of diminishing returns.
