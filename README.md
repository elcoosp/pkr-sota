<div align="center">
  <img src="docs/logo.png" alt="pkr-sota Logo" width="200"/>

  # pkr-sota

  *A No-Limit Texas Hold'em CFR solver that trains on a Mac Mini M1 and ships a memory-mapped blueprint for sub-millisecond lookup on a cheap VPS.*

  [![Rust](https://img.shields.io/badge/Rust-2021-000000?style=flat-square&logo=rust)](https://www.rust-lang.org)
  [![Crates](https://img.shields.io/badge/Crates-11-6F4E37?style=flat-square)](#workspace)
  [![Algorithm](https://img.shields.io/badge/Algorithm-DCFR%20%2B%20PCFR%2B-4B32C3?style=flat-square)](#cfr-algorithm)
  [![Throughput](https://img.shields.io/badge/Throughput-~27K%20it%2Fs%20%40%20M1-00BFFF?style=flat-square)](#performance)
  [![Runtime](https://img.shields.io/badge/Runtime-Memory%20Mapped-228B22?style=flat-square)](#runtime-artifact)
  [![Lookup p99](https://img.shields.io/badge/Lookup%20p99-%3C%201%20ms-333333?style=flat-square)](#performance)
  [![Smoke Test](https://img.shields.io/badge/Smoke%20Test-end--to--end-brightgreen?style=flat-square)](#quickstart)
  [![Tests](https://img.shields.io/badge/Tests-98%20passing-brightgreen?style=flat-square)](#project-status)

  ⭐ If you like this project, star it on GitHub — it helps a lot!

  [Overview](#overview) • [Architecture](#architecture) • [Quickstart](#quickstart) • [Performance](#performance) • [Docs](#docs)

</div>

---

A Rust workspace that takes a poker game from raw `Card` enums to a queried strategy in production. It implements external-sampling MCCFR with DCFR discounting and PCFR+ momentum, a k-means hand-strength abstraction, a compact `i32`/`i64` regret table, rayon-batched parallel training, atomic checkpoint rotation, and a runtime that `mmap`s the exported blueprint and binary-searches it on the hot path.

> [!NOTE]
> pkr-sota is a research-grade NLHE solver under active development. The end-to-end pipeline is functional today and verified by `./smoke.sh`: precompute → train → export → load → query. See [Project Status](#project-status) for what's wired up vs. scaffolded.

## Overview

The pipeline is split in two: a **training side** that runs on a beefy machine (Mac Mini M1 in the reference setup), and a **runtime side** that loads the exported blueprint on a cheap VPS or inside another application. The two halves only communicate through a single memory-mapped binary file.

```
   TRAINING (M1)                              RUNTIME (VPS or host app)
   ─────────────                              ─────────────────────────

   pkr-trainer binary                         pkr-runtime library
   ├── pkr-abstraction: load tables           ├── MmapReader
   ├── pkr-eval: mmap hand ranks              ├── SolverHandle
   ├── pkr-cfr: Trainer                       └── get_advice_fast(hash)
   │   ├── CompactRegretTable                     ↓
   │   │   ├── i32 regret + momentum           O(log n) binary search
   │   │   ├── i64 strategy_sum                over sorted key array
   │   │   └── thread-local idx cache
   │   ├── traverse (batched, 256 iters/sync)
   │   ├── flush_cpu_batch (sort + parallel)
   │   └── metrics (nodes, depth, cache, dedup)
   ├── pkr-export: write_blueprint
   └── produces:
       ├── blueprint.bin    (mmap'd key + CDF arrays)
       ├── metrics.csv      (live training health)
       └── stats.json       (post-training strategy analysis)
```

### Workspace

| Crate | Status | Purpose |
|---|---|---|
| `pkr-contracts` | current | Trait boundaries: `Evaluator`, `AbstractionBuilder`, `BlueprintProvider`, `GameRules` |
| `pkr-core` | current | `Card`, `Deck`, `GameState`, `NlheRuleset`, stack-allocated `legal_actions_into` |
| `pkr-eval` | current | `NlheEvaluator` (scalar) and `TableEvaluator` (mmap'd lookup). Combinadic unrank helpers. |
| `pkr-abstraction` | current | `KMeansAbstraction`, `calculate_ehs`, `pkr-abstraction-precompute` binary |
| `pkr-cfr` | current | `Trainer`, `CompactRegretTable`, `traverse`, `dcfr`, `metrics`. Batched parallel training. |
| `pkr-export` | current | `write_blueprint`, `FileHeader`, `fmph`, `translate` |
| `pkr-runtime` | current | `MmapReader`, `SolverHandle` — the VPS-side library |
| `pkr-testgames` | current | Kuhn poker harness for measuring CFR variants |
| `pkr-trainer` | current | CLI binary that orchestrates the whole pipeline |
| `pkr-exploit` | unwired | Opponent modeling overlay; defined, not integrated |
| `pkr-fuzz` | unwired | Rules fuzzing + eval harness; defined, not integrated |

## Architecture

The hot path is built around three deliberate choices:

- **Combinadic indexing everywhere.** Preflop uses `(52 choose 2)`, flop uses `(52 choose 5) × 10` hole-masks, turn uses `(52 choose 6) × 15` masks. Memory-dense, branch-light, no hashing on the lookup path.
- **Packed regret table.** `i32` regret + `i64` strategy-sum per infoset, fixed-point at scale 1000. No per-cell bookkeeping, no `Vec` allocations. A thread-local `idx` cache means the `get_or_create_idx` CAS loop almost never hits after warm-up.
- **256-iteration batching.** Each rayon dispatch runs 16 local iterations that buffer into thread-local arrays, then one sort + parallel merge + flush. This amortises the serial merge step and is what gave the project its first positive parallel scaling.

### CFR Algorithm

- **Regret update**: `r ← max(0, w·r⁺ + w·r⁻ + Δ)` where `w = t^p/(t^p+1)` for `t ≥ τ=1000`, else `w = 1` (canonical DCFR, Brown & Sandholm 2019).
- **Momentum**: PCFR+ (Farina, Kroer, Sandholm 2021): `Δ` is a smoothing of the raw regret delta over iterations.
- **Strategy accumulator**: unweighted own-reach sum, `i64` fixed-point at scale 1000.
- **Batching**: 256 logical iterations per rayon dispatch. Each chunk runs 16 iterations locally, buffers accumulate, then one merge + flush at the end.

> [!NOTE]
> The canonical discount factor is bounded in `[0.5, 1)`. In `f32`, `t^p + 1` rounds to `t^p` once `t^p` exceeds ~8.4e6, so the factor saturates to exactly `1.0` and behaves like vanilla CFR for most of a real training run. An earlier `RatioPower` formula was removed after the Kuhn harness proved it overflows `f32` around `t=3000`. See `docs/status.md` for details.

### Runtime Artifact

`blueprint.bin` layout (see `crates/pkr-export/src/writer.rs`):

```
FileHeader          (32 bytes: magic, version, variant, count, k, hash_algo)
key_count: u32
cdf_size:  u32
keys:      u64 × key_count         (sorted, for binary search)
cdf:       u8  × cdf_size          (K bytes per key, monotonic CDF)
```

`pkr-export` also builds an FMph (minimal perfect hash) structure but the runtime currently uses binary search over the sorted key array. Both layouts are correct; only one is on the hot path.

## Quickstart

### Smoke test

```bash
./smoke.sh
```

Builds tiny precompute artifacts, trains 10 iterations, exports a blueprint, loads it through `pkr-runtime`, and queries it. Verifies the whole chain works in ~2–5 minutes (cold) or seconds (warm).

### Production-scale profile

```bash
./proftest.sh
```

Generates realistic abstraction tables (k=64, flop table 26M entries), trains 100K iterations, and writes:

- `.proftest/metrics.csv` — time series of CFR health + timing
- `.proftest/stats.json` — end-of-run snapshot + strategy analysis + 200 sampled infosets
- `.proftest/blueprint.bin` — the actual artifact

### Throughput benchmark

```bash
./bench.sh   # THREADS_LIST="1 2 4 8" SECONDS_PER_RUN=15 by default
```

Iterates thread counts, prints `BENCH` lines you can diff. `it/s × 86400 = iterations per day`. Level-A arena-playable is roughly `1e6–1e7` iterations.

> [!TIP]
> Ctrl-C is safe at any point during training. Rerunning `./run.sh` resumes from `train.ckpt` if it exists — checkpoints use atomic `.tmp → .prev → final` rotation so a crash mid-write can never corrupt the main checkpoint file.

### Real training run

```bash
# Edit run.sh: set ITERATIONS=10000000
./run.sh
```

### CLI flags

The `pkr-trainer` binary exposes the whole pipeline through `clap`. The flags that matter:

| Flag | Default | Purpose |
|---|---|---|
| `--iterations <N>` | `100000` | Total iterations before auto-export. |
| `--threads <N>` | autodetect | Rayon pool size. 32 MiB stack per worker. |
| `--capacity <N>` | `5_000_000` | Initial `CompactRegretTable` slot count. |
| `--checkpoint <path>` | off | Enables rolling checkpoint save. |
| `--checkpoint-every <N>` | `10000` | Iterations between checkpoint writes. |
| `--metrics-csv <path>` | off | Per-interval row: regret, dedup, cache, depth, timing. |
| `--stats-json <path>` | off | End-of-run summary: snapshot + cumulative metrics + strategy analysis + sampled infosets. |
| `--report-every <N>` | `10000` | Iterations between progress + CSV rows. |
| `--eval-every <N>` | `0` (off) | Sampled best-response exploitability in milli-big-blinds per game. |
| `--eval-deals <N>` | `2000` | Deals sampled per exploitability check. Accuracy ~ `1/sqrt(deals)`. |
| `--bench-seconds <N>` | `0` (off) | Time-bounded benchmark mode (max iters in `bench_seconds` window). |

`pkr-abstraction-precompute` ships subcommands: `hand_ranks`, `centroids`, `preflop`, `flop`, `turn`, `river`, `flow`, `all7`, plus `abs5`/`abs6`/`all4`/`all6`/`all8` (variants of the abstraction-table generator).

## Performance

Measured on Mac Mini M1, 8 threads, k=64 centroids:

```
iter 5120/100000  | infosets: 110091  | 15196.1 it/s | cache_hit=0.584
iter 51200/100000 | infosets: 374078  | 21159.3 it/s | cache_hit=0.873
iter 100000/100000| infosets: 472881  | 27772.9 it/s | cache_hit=0.919
```

| Metric | Value |
|---|---|
| Training throughput (8 threads, k=64) | ~27,000 it/s steady state |
| Iterations per day | ~2.3 billion |
| Iterations for arena-playable (roadmap target) | ~10 million |
| Wall time for 10M iterations | ~6 minutes |
| Init time (load tables, allocate table) | 0.7 s |
| Runtime lookup (p99) | < 1 ms |
| Runtime memory (blueprint mmap'd) | file size + ~10 MB |

At `10⁷` iterations and 5M infosets, working set is ~600 MB. At `10⁸` iterations and 50M infosets, working set exceeds 3 GB and throughput degrades to roughly 15–20K it/s as the `papaya` map and regret arrays stop fitting in cache.

## Project Status

| Layer | State | Notes |
|---|---|---|
| Precompute (`hand_ranks`, `centroids`, `flop`/`turn`/`river` buckets, abstraction tables) | working | All subcommands wired up. `flow` is the main entry; `run.sh` calls the rest. |
| Training (`pkr-cfr`) | working | DCFR + PCFR+, batched parallel, cache-hit > 0.9 at steady state. |
| Checkpointing | working | Atomic `.tmp → .prev → final` rotation; resume on next launch. |
| Live metrics | working | CSV row + `eprintln!` summary per `--report-every`. Includes dedup ratio, depth histogram, cache hit. |
| Strategy analysis | working | End-of-run JSON: pure/mixed count, entropy histogram, dominant-action counts, 200 sampled infosets with raw regret + strategy vectors. |
| Sampled exploitability | working | `--eval-every` triggers `pkr_exploit::best_response::sampled_exploitability` — milli-big-blinds per game. |
| Export (`write_blueprint`) | working | Defensively sorts keys, normalises CDF via `get_average_strategy_into`. |
| Runtime lookup | working | `SolverHandle` re-exported from `pkr-runtime` crate root. Binary search over sorted keys. |
| `pkr-exploit` (full opponent modeling) | partial | Sampled exploitability works; full overlay not integrated into the trainer loop. |
| `pkr-fuzz` (rules fuzzing) | scaffolded | Defined, not integrated. |
| DCFR discount stability at `t > 8.4e6` | known issue | `f32` saturates `t^p + 1` → `t^p`; behaves as vanilla CFR past that point. Documented in `docs/status.md`. |

## Docs

See `docs/INDEX.md` for the full list with current/historical status.

- `docs/status.md` — current state of the codebase and known issues
- `docs/arch-overview.md` — architecture and design decisions
- `docs/spec/architecture.md` — Level 3 architectural specification
- `docs/spec/bst.md` — Level 4 behavioral specifications and test plan
- `docs/pkr-sota-winning-roadmap.md` — roadmap (some items done, some pending)
- `docs/tasks/` — machine-readable task definitions (historical)
- `docs/archive/` — superseded docs, kept for reference
