# pkr-sota 🃏⚡

A No-Limit Texas Hold'em solver for **Mac Mini M1 (training)** that exports a
compact, memory-mapped blueprint for **sub-millisecond lookup on a cheap VPS
or embedded in another application**.

Built in Rust. Training uses external-sampling CFR with a bounded
discount and PCFR+ momentum. The runtime is a library, not a server.

---

## What actually works

End-to-end, verified by `./smoke.sh`:

```
precompute hand ranks  ->  precompute abstraction tables
                       ->  train blueprint (CFR)
                       ->  export memory-mapped binary
                       ->  load from pkr-runtime and query
```

Measured on Mac Mini M1, 8 threads, k=64 centroids:

```
iter 5120/100000  | infosets: 110091  | 15196.1 it/s | cache_hit=0.584
iter 51200/100000 | infosets: 374078  | 21159.3 it/s | cache_hit=0.873
iter 100000/100000| infosets: 472881  | 27772.9 it/s | cache_hit=0.919
```

**Steady-state throughput: ~27,000 iterations/sec.** No non-finite regrets,
no crashes, no manual bookkeeping.

- 98 tests pass across 11 binaries, 1 ignored.
- `./smoke.sh` runs the whole chain in ~2-5 minutes (cold).
- `./proftest.sh` runs a production-scale pass in ~5 seconds (warm) and
  emits `metrics.csv` + `stats.json` for offline analysis.

---

## Architecture

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
| `pkr-contracts` | **current** | Trait boundaries: `Evaluator`, `AbstractionBuilder`, `BlueprintProvider`, `GameRules` |
| `pkr-core` | **current** | `Card`, `Deck`, `GameState`, `NlheRuleset`, stack-allocated `legal_actions_into` |
| `pkr-eval` | **current** | `NlheEvaluator` (scalar) and `TableEvaluator` (mmap'd lookup). Combinadic unrank helpers. |
| `pkr-abstraction` | **current** | `KMeansAbstraction`, `calculate_ehs`, `pkr-abstraction-precompute` binary |
| `pkr-cfr` | **current** | `Trainer`, `CompactRegretTable`, `traverse`, `dcfr`, `metrics`. Batched parallel training. |
| `pkr-export` | **current** | `write_blueprint`, `FileHeader`, `fmph`, `translate` |
| `pkr-runtime` | **current** | `MmapReader`, `SolverHandle` — the VPS-side library |
| `pkr-testgames` | **current** | Kuhn poker harness for measuring CFR variants |
| `pkr-trainer` | **current** | CLI binary that orchestrates the whole pipeline |
| `pkr-exploit` | unwired | Opponent modeling overlay; defined, not integrated |
| `pkr-fuzz` | unwired | Rules fuzzing + eval harness; defined, not integrated |

### CFR algorithm

- **Regret update**: `r ← max(0, w·r⁺ + w·r⁻ + Δ)` where `w = t^p/(t^p+1)`
  for `t ≥ τ=1000`, else `w = 1` (canonical DCFR, Brown & Sandholm 2019).
- **Momentum**: PCFR+ (Farina, Kroer, Sandholm 2021): `Δ` is a smoothing of
  the raw regret delta over iterations.
- **Strategy accumulator**: unweighted own-reach sum, i64 fixed-point at
  scale 1000.
- **Batching**: 256 logical iterations per rayon dispatch. Each chunk runs
  16 iterations locally, buffers accumulate, then one merge + flush at the
  end. This amortizes serial overhead and gave the first positive parallel
  scaling in the project's history.

> **Note on DCFR:** the canonical discount factor is bounded in [0.5, 1).
> In f32, `t^p + 1` rounds to `t^p` once `t^p` exceeds ~8.4e6, so the
> factor saturates to exactly 1.0 and behaves like vanilla CFR for most of
> a real training run. See `docs/status.md` for details. An earlier
> `RatioPower` formula was removed after the Kuhn harness proved it
> overflows f32 around t=3000.

### Runtime artifact

`blueprint.bin` layout (see `crates/pkr-export/src/writer.rs`):

```
FileHeader          (32 bytes: magic, version, variant, count, k, hash_algo)
key_count: u32
cdf_size:  u32
keys:      u64 × key_count         (sorted, for binary search)
cdf:       u8  × cdf_size          (K bytes per key, monotonic CDF)
```

`pkr-export` also builds an FMph (minimal perfect hash) structure but the
runtime currently uses binary search over the sorted key array. Both
layouts are correct; only one is on the hot path.

---

## Quickstart

### Smoke test (2-5 min cold, seconds warm)

```bash
./smoke.sh
```

Builds tiny precompute artifacts, trains 10 iterations, exports a
blueprint, loads it through `pkr-runtime`, queries it. Verifies the whole
chain works.

### Production-scale profile (~5 s warm, ~3 min cold)

```bash
./proftest.sh
```

Generates realistic abstraction tables (k=64, flop table 26M entries),
trains 100K iterations, writes:
- `.proftest/metrics.csv` — time series of CFR health + timing
- `.proftest/stats.json` — end-of-run snapshot + strategy analysis + samples
- `.proftest/blueprint.bin` — the actual artifact

### Real training run

```bash
# Edit run.sh: set ITERATIONS=10000000
./run.sh
```

Ctrl-C is safe. Rerunning resumes from `train.ckpt` if it exists.

---

## Performance

| Metric | Value |
|---|---|
| Training throughput (8 threads, k=64) | ~27,000 it/s steady state |
| Iterations per day | ~2.3 billion |
| Iterations for arena-playable (roadmap target) | ~10 million |
| Wall time for 10M iterations | ~6 minutes |
| Init time (load tables, allocate table) | 0.7 s |
| Runtime lookup (p99) | < 1 ms |
| Runtime memory (blueprint mmap'd) | file size + ~10 MB |

At 10⁷ iterations and 5M infosets, working set is ~600 MB. At 10⁸ iterations
and 50M infosets, working set exceeds 3 GB and throughput degrades to
roughly 15-20K it/s as the papaya map and regret arrays stop fitting in
cache.

---

## Docs

See `docs/INDEX.md` for the full list with current/historical status.

- `docs/status.md` — current state of the codebase and known issues
- `docs/arch-overview.md` — architecture and design decisions
- `docs/spec/architecture.md` — Level 3 architectural specification
- `docs/spec/bst.md` — Level 4 behavioral specifications and test plan
- `docs/pkr-sota-winning-roadmap.md` — roadmap (some items done, some pending)
- `docs/tasks/` — machine-readable task definitions (historical)
- `docs/archive/` — superseded docs, kept for reference

---

## License

TBD
