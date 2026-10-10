<div align="center">
  <img src="docs/logo.png" alt="pkr-sota logo" width="140"/>

  # pkr-sota

  *An end-to-end heads-up No-Limit Hold'em poker AI in Rust — precompute, train, export, and query a CFR blueprint on a single machine.*

  [![Rust](https://img.shields.io/badge/Rust-2021-000000?style=flat-square&logo=rust)](https://www.rust-lang.org)
  [![Workspace](https://img.shields.io/badge/workspace-19%20members-3C8CBB?style=flat-square)](#the-workspace)
  [![Tests](https://img.shields.io/badge/tests-460%2B-2EA043?style=flat-square)](#continuous-integration)
  [![CI](https://img.shields.io/badge/CI-9%20workflows-8A63D2?style=flat-square)](https://github.com/elcoosp/pkr-sota/actions)
  [![Blueprint](https://img.shields.io/badge/blueprint%20format-v4-F3A73B?style=flat-square)](crates/pkr-export/src/header.rs)

  [Overview](#overview) · [Workspace](#the-workspace) · [How it works](#how-it-works) · [Quickstart](#quickstart) · [Configuration](#configuration) · [CI](#continuous-integration) · [Status](#project-status) · [Docs](#documentation)
</div>

## Overview

`pkr-sota` is a Cargo workspace that takes heads-up No-Limit Hold'em from raw `Card` bytes to a queried strategy, split by one design constraint: **train on one beefy machine, answer queries anywhere**. An offline half builds hand-strength tables and runs counterfactual-regret-minimization training; the result is frozen into a single self-describing `blueprint.bin` that the online half loads with `mmap` and answers in nanoseconds — no server, no heap allocations, no heap of dependencies.

The pipeline has four stages, and each one is its own crate boundary:

```text
                 training host (offline)
┌──────────────────────────────────────────────────────────────┐
│  pkr-abstraction-precompute                                  │
│    hand_ranks.bin · centroids · per-street bucket tables     │
│                         │                                    │
│                         ▼                                    │
│  pkr-trainer ── pkr-cfr  (external-sampling MCCFR + DCFR)    │
│                         │                                    │
│                         ▼                                    │
│  pkr-export ──▶ blueprint.bin  (sorted keys + u8 CDFs)       │
└──────────────────────────────────────────────────────────────┘
                 runtime host (online)
┌──────────────────────────────────────────────────────────────┐
│  pkr-runtime                                                 │
│    mmap → branchless binary search → advice    (p50 ≈ 42 ns) │
│    RuntimeSession: range tracking · off-tree bet translation │
│    river-only subgame search (bounded · CFR-D gadget)        │
└──────────────────────────────────────────────────────────────┘
```

> [!NOTE]
> "sota" is the destination, not a claim. The project's own assessments in [`docs/status.md`](docs/status.md) grade the current bot honestly, and every experiment document records its measurement caveats — including the negative results. See [Project status](#project-status).

## Features

- **End-to-end on one machine.** `./smoke.sh` runs the full pipeline — hand-rank table, centroids, per-street bucket tables, training, export, reload — and finishes with a release-mode integration test that loads the blueprint through `pkr-runtime`.
- **Deterministic by construction.** Same seed and inputs produce a byte-identical `blueprint.bin` at any thread count; the `golden` CI workflow fails on any drift from stored SHA-256 hashes.
- **Nanosecond queries.** The runtime `mmap`s the blueprint and answers lookups with a branchless binary search — p50 ≈ 42 ns, p99 ≈ 84 ns against a large blueprint on an M1 (see [`docs/status.md`](docs/status.md)).
- **Safety-first search.** River subgame solving ships only in provably-safe modes: a bounded variant that blends back until the opponent's best response is no better than against the blueprint, and a CFR-D gadget (Burch et al., 2014) that grants the opponent a terminate option paying blueprint continuation value.
- **Honest instrumentation.** Sampled best response with held-out lower bounds, myopic local best response, duplicate-poker tournaments with common random numbers, and exact-enumeration convergence canaries on Kuhn and Leduc.
- **Battle-tested.** 460+ `#[test]` functions across the workspace, Miri on the core crates, fuzz targets over the blueprint loader and game-state transitions, nightly criterion benchmarks pushed to Bencher.

## The workspace

19 members: 12 library crates, 1 binary, 6 benchmark crates (all paths relative to the repo root).

| Member | Role |
|---|---|
| `crates/pkr-core` | Game model: cards, deck, zero-alloc `GameState`, NLHE rules, the 7-bucket action abstraction, infoset signatures |
| `crates/pkr-contracts` | Trait boundaries shared by every layer — `Evaluator`, `AbstractionBuilder`, `BlueprintProvider` — plus FNV-1a infoset hashing |
| `crates/pkr-eval` | Hand evaluation: a reference evaluator, a C(52,5)-entry mmap lookup table, and `Fast7Evaluator` (bit-identical by design, enforced by differential tests) |
| `crates/pkr-abstraction` | Card abstraction: Monte-Carlo EHS/EHS², equity histograms, k-means bucket tables, and the `pkr-abstraction-precompute` binary |
| `crates/pkr-cfr` | Training engine: external-sampling MCCFR with DCFR discounting over a lock-free `CompactRegretTable` |
| `crates/pkr-export` | `blueprint.bin` writer/reader: versioned header, abstraction fingerprint, quantized u8 CDFs |
| `crates/pkr-runtime` | Query side: mmap + branchless binary search, `RuntimeSession`, opponent range tracking, live bet translation |
| `crates/pkr-subgame` | Subgame solving on the public tree; a vector-form solver carrying full 1326-hand ranges; the CFR-D safety gadget |
| `crates/pkr-exploit` | Exploitability instruments: sampled best response (held-out lower bound) and myopic local best response |
| `crates/pkr-fuzz` | Differential game-rules fuzzing against a reference implementation, plus duplicate-poker tournament and arena harnesses |
| `crates/pkr-testgames` | Kuhn poker harness with exact best-response enumeration (Nash value −1/18) |
| `crates/pkr-leduc-check` | Leduc hold'em convergence gate over the production CFR path — a CI canary for the trainer's numerics |
| `binaries/pkr-trainer` | The `pkr-trainer` CLI that orchestrates the whole pipeline |
| `benches/` | Six criterion bench crates: hashing, eval, runtime lookup, CFR table ops, abstraction, core primitives |

## How it works

### Abstraction

Texas Hold'em is far too large to solve directly, so both axes of the game get compressed. On the **action** side, every bet is bucketed into one of 7 discrete actions — fold, check/call, four pot-relative bet sizes, or jam — with the mapping defined once in `pkr-core/src/abstraction.rs` and a CI gate (`ci/scripts/check-no-hardcoded-k.sh`) that rejects any hard-coded copy of that count. On the **card** side, preflop hands map to the exact 169 strategic classes, while flop, turn, and river situations map through precomputed bucket tables built by k-means over equity features (EHS/EHS² and equity histograms, compared with an earth-mover-distance metric). A 40-byte abstraction fingerprint describing every one of these choices is written into checkpoints and blueprints, and the loader refuses artifacts whose fingerprint doesn't match the code.

### Training

`pkr-trainer` runs external-sampling MCCFR with DCFR discounting (α = 1.5, β = 0, γ = 2), RM+ flooring, and optional PCFR+ momentum (off by default — the docs recommend keeping it off). Regrets live in a `CompactRegretTable`: a lock-free concurrent map onto flat atomic planes, 112 bytes per infoset at K=7, with thread-local lookup caches and rayon-parallel traversal batches. Checkpointing is rolling and atomic (`.tmp → .prev → final`), so a crash mid-write cannot corrupt the main checkpoint, and `--checkpoint` resumes from wherever training stopped. Hyperparameters are read from `PKR_*` environment variables exactly once per process and pinned by regression tests.

### The blueprint file

`pkr-export` writes a single self-describing file: an 8-byte `PKRSOTA1` magic, format version (v4), a per-street bet-sizing anchor block, the abstraction fingerprint, then ascending `u64` infoset keys and one quantized `u8` cumulative-distribution byte-slice per key. Writing is atomic (`.tmp` + rename), keys are sorted defensively, and the runtime validates magic, version, hash algorithm, and counts before trusting anything. The reader accepts v2–v4; anything newer is rejected loudly rather than misread.

### Querying

`pkr-runtime` maps the blueprint read-only and answers `SolverHandle::get_advice_fast(hash)` with a branchless binary search over the sorted keys — no allocator on the hot path. On a miss, a fixed mild fallback policy is returned rather than garbage. Above the blueprint sits `RuntimeSession`: it tracks a 1326-hand posterior over the opponent's holdings from observed actions, translates off-tree bet sizes into the nearest in-tree bucket (pseudo-harmonic translation, Ganzfried & Sandholm 2013), and optionally runs river-only subgame search inside a wall-clock budget (default 2000 ms).

### Playing above the blueprint

Search is shipped river-only and safe-by-default. The bounded mode solves the subgame, measures the opponent's best response against it, and blends back toward the blueprint until that response is no better than against the blueprint itself. The gadget mode instead wraps the solve in a CFR-D terminate option paying blueprint continuation value, which bounds exploitability independent of range accuracy. Turn search costs ~45 s per decision on the M1 and stays offline; river decisions fit comfortably in the live budget.

## Quickstart

Prerequisites: a stable Rust toolchain (edition 2021, no pinning), a Unix-ish shell, and roughly 1 GB of disk for generated tables. Note that `.cargo/config.toml` builds everything with `-C target-cpu=native` by design — these binaries are meant for the machine that built them.

```bash
git clone https://github.com/elcoosp/pkr-sota
cd pkr-sota

# End-to-end proof: precompute → train → export → load
./smoke.sh
```

`smoke.sh` caches its artifacts under `outputs/v0-smoke/` and re-runs incrementally; `SMOKE_FRESH=1 ./smoke.sh` starts from scratch. It also asserts exact byte sizes on the generated tables (e.g. the 305 MB turn-abstraction table), so a truncated run fails loudly.

Train a real blueprint:

```bash
VERSION=v1 ITERATIONS=1000000 ./run.sh   # defaults: 30M iterations, checkpoints every 5M
```

`run.sh` handles precompute (rebuilding only on `REBUILD=1`), launches training with periodic sampled-exploitability evals behind promotion gates (an improvement must clear both a margin and 2 standard errors), and writes `blueprint.bin`, `train.ckpt`, `metrics.csv`, and `stats.json` under `outputs/$VERSION/`. Interrupt it freely — SIGINT/SIGTERM are handled and the checkpoint resumes.

Query the result:

```bash
# Post-deploy health check: latency percentiles and hit rate, meaningful exit codes
cargo run --release -p pkr-runtime --bin health -- outputs/v1/blueprint.bin

# Line-protocol bot over stdin/stdout: JSON requests in, one action token out (f | k | c | b<chips>)
cargo run --release -p pkr-runtime --bin pkr-bot-pipe -- outputs/v1/blueprint.bin outputs/v1
```

Common maintenance commands (via [`justfile`](justfile)):

| Command | What it does |
|---|---|
| `just fast` | fmt + clippy `-D warnings` + nextest — the inner development loop |
| `just smoke` / `just smoke-fresh` | end-to-end pipeline (cached / from scratch) |
| `just train` | `./run.sh` with environment-driven configuration |
| `just bench` | thread-scaling throughput benchmark |
| `just prof` | production-scale profiled run (100k iterations) |
| `just check` | fmt check + clippy + tests, mirroring CI's `fast` workflow |

## Configuration

All hyperparameters are environment variables read once per process (set them before launching):

| Variable | Default | Effect |
|---|---|---|
| `PKR_STACK_BB` | `100` | Starting stack in big blinds (clamped to 20–400) |
| `PKR_RM_PLUS` | on | RM+ floor of sampled regrets at zero |
| `PKR_MOMENTUM` | off | PCFR+ momentum term (project docs recommend off) |
| `PKR_DCFR_ALPHA` | `1.5` | Positive-regret discount exponent |
| `PKR_EXPLORE_EPSILON` | `0.01` | Uniform exploration mixed into opponent sampling |
| `PKR_ALLOW_EHS_FALLBACK` | off | Permit the slow Monte-Carlo fallback when a bucket table misses (trainer otherwise aborts) |
| `PKR_VSEARCH` | off | Enable river subgame search on top of the blueprint |
| `PKR_VSEARCH_BUDGET_MS` | `2000` | Wall-clock budget per search decision |
| `EHS_SAMPLES` | `1000` | Monte-Carlo deals per EHS estimate during precompute |
| `PKR_EMIT_FMPH` | off | Emit the optional minimal-perfect-hash tail in the blueprint |

> [!TIP]
> `cargo run --release -p pkr-trainer -- --help` documents all ~35 trainer flags — capacity, evaluator choice (`table` or `fast7`), checkpoint intervals, eval scheduling, promotion gates, plateau stopping, seeding, and JSON logging.

## Continuous integration

Nine GitHub Actions workflows guard the repo; the full definitions live in [`.github/workflows/`](.github/workflows/).

| Workflow | Trigger | What it checks |
|---|---|---|
| `fast` | every push to `main`, all PRs | fmt, clippy `-D warnings`, nextest, doctests |
| `golden` | PRs touching solver crates | 100k-iteration seed-42 training must reproduce stored SHA-256 hashes (blueprint at any thread count, checkpoint at 1 thread) |
| `smoke` | PRs + daily schedule | the full end-to-end pipeline plus artifact byte-size assertions |
| `bench` | nightly | six criterion suites, thread scaling, and a Kuhn experiment, pushed to Bencher with PR baselines |
| `proftest` | nightly | a 50k-iteration profiled training run with metrics pushed to Bencher |
| `weekly` | weekly | coverage, dhat heap profiling, two 5-minute fuzz runs, Miri on `pkr-core` + `pkr-cfr`, flamegraph |
| `audit` | daily + dependency PRs | `cargo audit` + `cargo deny` (RUSTSEC advisories, licenses, bans) |
| `pr-comment` | PRs | criterion A/B against `main`, posted as a diff-table comment |
| `commit-bench` | after `bench` | archives nightly benchmark results to the `perf-history` branch |

The deterministic-training gate deserves a mention: `ci/golden-training.sha256` stores reference hashes for a fixed 100k-iteration run, and any unintended change to the trainer's numerics shows up as a hash mismatch.

## Project status

What is verified working today:

- The full pipeline (precompute → train → export → load → query), exercised by `smoke.sh` and held in place by the golden-hash CI gate.
- Deterministic training: identical seed and inputs yield a byte-identical blueprint regardless of thread count.
- Runtime lookups at p50 ≈ 42 ns / p99 ≈ 84 ns (M1, large blueprint — [`docs/status.md`](docs/status.md)).
- River-only bounded search beats the bare blueprint by ≈ +1.03 chips/deal on average across four decorrelated seeds (t ≥ 4.1), with the safety bound (search best-response ≤ blueprint best-response) passing in test.
- The production CFR path converges on Leduc hold'em to well under the CI gate's 70 milli-chips/game threshold (~21 measured at 2M iterations).
- 460+ unit/integration tests, Miri-clean core crates, fuzzed blueprint loader and state transitions.

> [!IMPORTANT]
> Three honesty rules the project holds itself to, which apply to anything you read here:
>
> 1. **This is not a superhuman bot.** The project's own reviews place it far from competitive HUNL solvers; the realistic near-term goal is a strategy that is *provably not trivially exploitable*.
> 2. **Absolute exploitability numbers are inflated.** The in-sample estimator behind the historical numbers in `docs/experiments/` is known-biased; only seeded, relative A/B comparisons between runs are meaningful.
> 3. **K=7 invalidated history.** The 7-bucket action abstraction landed on 2026-10-10; every earlier checkpoint is unloadable by design, K=6 and K=7 are different abstract games whose numbers must not be compared, and a K=7 re-baseline is the current open work.

The [`docs/experiments/`](docs/experiments) directory is a deliberate record of dead ends too — finer river bucketing, k=400 abstraction, turn-time search, PCFR+ momentum, and variance-reduction schemes all have negative or inconclusive verdicts written up alongside the wins.

## Documentation

- [`docs/INDEX.md`](docs/INDEX.md) — annotated index of every document, with current-vs-historical status
- [`docs/status.md`](docs/status.md) — current state of the codebase, verified numbers, known issues
- [`docs/arch-overview.md`](docs/arch-overview.md) — architecture and design decisions
- [`docs/spec/architecture.md`](docs/spec/architecture.md) and [`docs/spec/bst.md`](docs/spec/bst.md) — layered specification
- [`docs/roadmap/PLAN_STATUS_2026-10-10.md`](docs/roadmap/PLAN_STATUS_2026-10-10.md) — the latest plan status
- [`docs/experiments/VERIFIED_FINDINGS.md`](docs/experiments/VERIFIED_FINDINGS.md) — what is actually confirmed, with measurement context
- [`docs/handoffs/`](docs/handoffs) and [`docs/playbooks/`](docs/playbooks) — session handoffs and working procedures
- [`CHANGELOG.md`](CHANGELOG.md) — release notes
