<div align="center">
  <img src="docs/logo.png" alt="pkr-sota Logo" width="200"/>

  # pkr-sota

  [![fast](https://github.com/elcoosp/pkr-sota/actions/workflows/fast.yml/badge.svg?branch=main)](https://github.com/elcoosp/pkr-sota/actions/workflows/fast.yml)
  [![smoke](https://github.com/elcoosp/pkr-sota/actions/workflows/smoke.yml/badge.svg?branch=main)](https://github.com/elcoosp/pkr-sota/actions/workflows/smoke.yml)
  [![audit](https://github.com/elcoosp/pkr-sota/actions/workflows/audit.yml/badge.svg)](https://github.com/elcoosp/pkr-sota/actions/workflows/audit.yml)
  [![bench](https://github.com/elcoosp/pkr-sota/actions/workflows/bench.yml/badge.svg)](https://github.com/elcoosp/pkr-sota/actions/workflows/bench.yml)
  [![proftest](https://github.com/elcoosp/pkr-sota/actions/workflows/proftest-ci.yml/badge.svg)](https://github.com/elcoosp/pkr-sota/actions/workflows/proftest-ci.yml)
  [![weekly](https://github.com/elcoosp/pkr-sota/actions/workflows/weekly.yml/badge.svg)](https://github.com/elcoosp/pkr-sota/actions/workflows/weekly.yml)

  *A Rust workspace that takes a poker game from raw `Card` enums to a queried strategy in production.*

  [![Rust](https://img.shields.io/badge/Rust-2021%20Edition-000000?style=flat-square&logo=rust)](https://www.rust-lang.org)
  [![Crates](https://img.shields.io/badge/Crates-11-6F4E37?style=flat-square)](#workspace)
  [![License](https://img.shields.io/badge/License-TBD-blue?style=flat-square)](#)

  ⭐ If you like this project, star it on GitHub — it helps a lot!

  [Overview](#overview) • [Workspace](#workspace) • [Architecture](#architecture) • [Quickstart](#quickstart) • [CLI](#cli) • [Abstraction](#abstraction-subsystem) • [Status](#project-status)

</div>

---

A Cargo workspace of 11 crates that implements an end-to-end poker pipeline: precompute hand-rank and k-means abstraction tables, train a regret-based strategy, export a binary blueprint, and query it at runtime. This README describes what's **verifiable from the actual code in this repository** — for performance numbers, algorithm internals, and crate APIs that live in source files not yet in this snapshot, see [Roadmap & unverified claims](#roadmap--unverified-claims).

> [!NOTE]
> The end-to-end pipeline is functional today and verified by `./smoke.sh` — precompute → train → export → load → query. The script is referenced in `CHANGELOG.md` as the project's end-to-end proof-of-concept (10 training iterations, blueprint export, `pkr-runtime` load).

## Overview

`pkr-sota` is split in two halves that communicate through a single binary file:

- **Training** (`pkr-trainer` binary): orchestrates abstraction loading, regret-minimisation training, and blueprint export. CLI built with `clap`, parallelism via `rayon`, logging via `tracing`.
- **Runtime** (`pkr-runtime` library, source not in this snapshot): loads the exported blueprint via `mmap` and exposes a query API.

The two halves share the `pkr-abstraction` crate for street-aware infoset hashing, `pkr-eval` for hand-rank lookup, and `pkr-contracts` for the trait boundaries between layers.

### Release profile

From the root `Cargo.toml` `[profile.release]`:

```toml
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
debug = true
strip = "symbols"
```

The workspace edition is `2021`, version `0.1.0`, authors `["pkr-sota"]`, resolver `2`.

## Workspace

The 11 crates declared in the root `Cargo.toml` `[workspace]`:

| Crate | Source in this snapshot? | Role (per `Cargo.toml` members list) |
|---|---|---|
| `crates/pkr-contracts` | no | Trait boundaries referenced by `pkr-abstraction` (`Evaluator`, `AbstractionBuilder`, `fnv1a`, `FNV_OFFSET`) |
| `crates/pkr-core` | no | Card/Deck/Game primitives |
| `crates/pkr-cfr` | no | Trainer + regret table (`Trainer::with_capacity`, `run_iterations_parallel`, `load_checkpoint`, `save_checkpoint`, `iteration`, `get_table`, `is_near_capacity`) |
| `crates/pkr-eval` | no | `NlheEvaluator` (scalar), `TableEvaluator::new(path)`, combinadic helpers (`choose`, `combinadic_unrank_{2,3,5,6,7}`) |
| `crates/pkr-abstraction` | **yes** | `KMeansAbstraction`, `calculate_ehs`, `pkr-abstraction-precompute` binary |
| `crates/pkr-export` | no | `write_blueprint(path, table, &keys)`, `FileHeader` |
| `crates/pkr-runtime` | no | `SolverHandle` (re-exported at crate root per `CHANGELOG.md`), `debug_keys()` |
| `crates/pkr-fuzz` | no | Scaffolded, not integrated |
| `crates/pkr-exploit` | no | `best_response::sampled_exploitability(table, abstraction, evaluator, deals, seed)` — sampled exploitability in milli-big-blinds per game |
| `crates/pkr-testgames` | no | Kuhn poker harness (per stale README — not in this snapshot) |
| `binaries/pkr-trainer` | **yes** | CLI binary that orchestrates the whole pipeline |

Crates whose source is **in this snapshot** are documented with verified API surfaces below. Crates whose source is **not in this snapshot** are documented with whatever API surface is *referenced* from the snapshot — claims marked "per `CHANGELOG.md`" or "referenced from `main.rs`" are not yet grounded in the implementation.

## Architecture

Verifiable from `binaries/pkr-trainer/src/main.rs`:

```text
pkr-trainer binary
├── TableEvaluator::new(--rank-table)              ← pkr-eval, mmap'd hand-rank lookup
├── load_centroids(--centroids) → CentroidStore    ← pkr-abstraction, bincode-deserialised
├── KMeansAbstraction::from_store(store, evaluator)
├── abstraction.load_street_centroids(street, path)  optional, streets 1..=3
├── abstraction.init_table(street, path)             optional, streets 0..=3, mmap'd
├── abstraction.load_flop_buckets(path)              optional
├── Trainer::with_capacity(abstraction, evaluator, capacity)
├── trainer.load_checkpoint(path)?                   if checkpoint file exists
├── loop { trainer.run_iterations_parallel(256);     ← ITERS_PER_SYNC = 256
│         report + CSV row every --report-every
│         sampled exploitability every --eval-every
│         save_checkpoint_rolling(path) every --checkpoint-every
│         break if trainer.is_near_capacity() }
├── trainer.get_table().get_keys() → sorted
├── write_blueprint(output, trainer.get_table(), &keys)
└── optional: stats JSON with strategy analysis + 200 sampled infosets
```

Key implementation facts verified from the source:

- **`ITERS_PER_SYNC: u32 = 256`** — the per-rayon-dispatch iteration batch size (`binaries/pkr-trainer/src/main.rs`).
- **32 MiB rayon worker stack** — `ThreadPoolBuilder::new().num_threads(n).stack_size(32 * 1024 * 1024).build_global()`.
- **Atomic checkpoint rotation** — `save_checkpoint_rolling` writes to `.tmp`, rotates the existing checkpoint to `.prev`, then renames `.tmp` to the final path. A crash mid-write can never corrupt the main checkpoint file.
- **Resumable training** — if `--checkpoint <path>` is set and the file exists at startup, the trainer calls `trainer.load_checkpoint(path)` and resumes from `trainer.iteration()`. A failure prints a warning and starts fresh.

## Quickstart

### Smoke test

```bash
./smoke.sh
```

Per `CHANGELOG.md`: end-to-end proof-of-concept — precompute, train 10 iters, export blueprint, load via `pkr-runtime`. The script uses absolute paths and cleans `.smoke/` before each run (also per `CHANGELOG.md`). Both `.smoke/` and `.proftest/` are gitignored.

### Throughput benchmark

```bash
./bench.sh
```

Verifiable from `bench.sh`:

- Default `THREADS_LIST="1 2 4 8"`, default `SECONDS_PER_RUN=15`.
- Requires `.smoke/turn_abstraction.bin` to exist (run `./smoke.sh` first).
- Builds `pkr-trainer` in release mode, sets `PKR_PHASE_PROFILE=1`, and invokes the trainer with `--bench-seconds`, `--threads`, `--capacity 10000000`, and the abstraction-table paths under `.smoke/`.
- Grep filter on stdout: `(Running with|BENCH|iter .*infosets|\[phase\])`.
- The script's own closing note: "level-A arena-playable is roughly 1e6-1e7 iterations."

### Real training run

```bash
./run.sh    # referenced from CHANGELOG.md (ITERATIONS variable)
```

`run.sh` and the `justfile` are referenced in `CHANGELOG.md` as updated with checkpoint flags and a clippy gate. Neither file is in this snapshot.

## CLI

The `pkr-trainer` binary exposes a `clap::Parser` CLI. Verified from `Cli` struct in `binaries/pkr-trainer/src/main.rs`:

| Flag | Default | Purpose |
|---|---|---|
| `--iterations <N>` | `30000000` | Total iterations before auto-export. 30M is the current sweet spot per `docs/experiments/v38-30M-sweetspot.md`. |
| `--output <path>` | `blueprint.bin` | Blueprint output path. |
| `--centroids <path>` | `centroids.bin` | Default centroids file (bincode-serialised `CentroidStore`). |
| `--flop_centroids <path>` | none | Optional per-street centroids. |
| `--turn_centroids <path>` | none | Optional per-street centroids. |
| `--river_centroids <path>` | none | Optional per-street centroids. |
| `--preflop-table <path>` | none | Optional mmap'd preflop abstraction table. |
| `--flop-table <path>` | none | Optional mmap'd flop abstraction table. |
| `--turn-table <path>` | none | Optional mmap'd turn abstraction table. |
| `--river-table <path>` | none | Optional mmap'd river abstraction table. |
| `--flop-buckets <path>` | none | Optional flop-bucket array (raw `u8`). |
| `--rank-table <path>` | `hand_ranks.bin` | `TableEvaluator` hand-rank lookup table. |
| `--evaluator <name>` | `table` | `table` (exact) or `fast7` (LUT). |
| `--threads <N>` | autodetect | Rayon pool size. 32 MiB stack per worker. |
| `--checkpoint <path>` | none | Enables rolling checkpoint save. |
| `--checkpoint-every <N>` | `500000` | Iterations between checkpoint writes. |
| `--capacity <N>` | `60000000` | Initial `CompactRegretTable` slot count. |
| `--bench-seconds <N>` | `0` (off) | Time-bounded benchmark mode. |
| `--metrics-csv <path>` | none | Per-interval CSV row. |
| `--stats-json <path>` | none | End-of-run JSON summary. |
| `--report-every <N>` | `1000000` | Iterations between progress + CSV rows. |
| `--iters-per-sync <N>` | `2048` | Iterations per rayon dispatch. |
| `--eval-every <N>` | `0` (off) | Sampled exploitability check interval. |
| `--eval-deals <N>` | `2000` | Deals sampled per exploitability check. |
| `--promote-gate <mbb>` | `3` | Allow a promoted reading to be up to this much worse than the best so far. |
| `--promote-min-sigma <N>` | `2` | Require an improvement over the best to clear N BR standard errors before promoting (winner's-curse guard). |
| `--stop-on-plateau <N>` | `0` | Stop after N consecutive evals without a new historical minimum. 0 = off. |
| `--seed <u64>` | `0x5EED_1F70` | Worker RNG seed. Same seed + same inputs = identical training run. |
| `--fresh` | off | Ignore any existing checkpoint. |
| `--log-json` | off | Emit progress and eval lines as one-line JSON on stderr. |

### Metrics CSV columns

Verified from the `writeln!` call in `main.rs`:

```
iter,wall_s,it_per_s,infosets,cap_pct,max_abs_regret,mean_abs_regret,
nonfinite,strat_mass,nodes,nodes_per_iter,avg_depth,max_depth,cache_hit_rate,
regret_in,regret_out,regret_dedup,strategy_applied,
traverse_ms,merge_ms,flush_ms,wall_ms
```

### Stats JSON shape

Verified from the `serde_json::json!` block in `main.rs`:

- `config`: `iterations`, `threads`, `capacity`, `iters_per_sync`, `report_every`, `start_iter`, `end_iter`, `stopped_early`.
- `wall_seconds`.
- `snapshot`: `infosets`, `capacity`, `capacity_pct`, `max_abs_regret`, `mean_abs_regret`, `nonfinite_count`, `strategy_sum_mass`.
- `cumulative_metrics`: `nodes`, `nodes_per_iteration`, `avg_depth`, `max_depth`, `cache_hit_rate`, `infosets_created`, `strategy_ops_pushed`, `strategy_ops_applied`, `regret_ops_input`, `regret_ops_unique`, `regret_dedup_ratio`, `batches`, `total_traverse_s`, `total_merge_s`, `total_flush_s`, `total_wall_s`, `depth_histogram`.
- `strategy_analysis`: `total`, `empty`, `pure`, `mixed`, `mean_entropy_bits`, `entropy_histogram_0p25bit`, `dominant_action_counts`, `nonzero_strategy_sum_cells`, `uniform_fallback`.
- `sample_infosets`: 200 entries, each `{ hash: "0x..", strategy: [f32], regrets: [f32] }`.

### `pkr-abstraction-precompute` subcommands

Verified from the `match args[1]` in `crates/pkr-abstraction/src/bin/precompute.rs`:

| Command | Args | What it does |
|---|---|---|
| `hand_ranks` | `[output]` (default `hand_ranks.bin`) | Evaluates all `(52 choose 5)` 5-card combinations with `NlheEvaluator` and writes `u32` ranks little-endian. |
| `centroids` | `[num_samples] [k] [rank_table_path] [output]` (defaults `1000`, `200`, `hand_ranks.bin`, `centroids.bin`) | Computes EHS/EHS² for all `(52 choose 2)` hole-cards, runs `simple_kmeans` (2D, 50 iters), serialises a `CentroidStore` via bincode. |
| `preflop` | `[centroids_path] [rank_table_path] [output]` | Per-hole-card nearest-centroid id → `u8` table. |
| `flop` | `[rank_table_path] [output] [k]` (default `k=64`) | 10-dim EHS histogram per flop, `kmeans_10d` clusters them, writes `u8` bucket per flop. |
| `turn` | `[centroids_path] [rank_table_path] [output] [num_samples]` | Per `(hole, board)` pair, 15 hole-masks × `(52 choose 6)` combos, nearest centroid → `u8`. |
| `river` | `[rank_table_path] [output] [k]` (default `k=256`) | 10-dim EHS histogram per 5-card board, `kmeans_10d`, `u8` bucket per board. |
| `flow` | `[centroids_path] [rank_table_path] [output]` | Abstraction table over `(52 choose 5) × 10` hole-masks → `u8`. |
| `all7` | `[rank_table_path] [output]` | EHS × 255 → `u8` for every `(52 choose 7)` combo. |
| `abs5` `abs6` `all4` `all6` `all8` | same as `flow` | Aliases for `flow` (variants of the abstraction-table generator). |

> [!NOTE]
- All abstraction tables use `u8` bucket ids; centroid count must be `<= 255` (asserted in `precompute.rs`).
- Flop and river buckets are derived from 10-dim EHS histograms (500 sampled deals per flop, 200 per river board).
- EHS sampling defaults to 1000 deals per call, configurable via the `EHS_SAMPLES` env var (read once via `OnceLock` in `ehs.rs`).

## Abstraction subsystem

This is the only crate with full source in the snapshot, so it's documented in detail.

### `ehs.rs` — Expected Hand Strength

```rust
pub fn calculate_ehs(hole: &[u8], board: &[u8], evaluator: &dyn Evaluator) -> (f32, f32)
```

- Returns `(ehs, ehs_sq)` — mean equity and mean equity squared over `num_samples()` Monte-Carlo deals.
- Zero heap allocations on the hot path: stack arrays `remaining[50]` and `full_board_buf[5]`, plus `partial_shuffle` from `rand`.
- Equity per deal: `1.0` win, `0.5` tie, `0.0` loss.
- Sample count: `EHS_SAMPLES` env var, default `1000`, cached in a `OnceLock<usize>`.

### `lib.rs` — `KMeansAbstraction`

```rust
pub struct KMeansAbstraction {
    centroids: HashMap<u8, Vec<(f32, f32)>>,   // per-street
    default_centroids: Vec<(f32, f32)>,
    tables: HashMap<u8, OnceLock<Mmap>>,         // street 0..=3, mmap'd
    flop_buckets: OnceLock<Vec<u8>>,
    evaluator: Arc<dyn Evaluator>,
}
```

Implements `AbstractionBuilder` (from `pkr-contracts`). The `get_infoset_hash(hole, board, history, street) -> u64` method:

1. Picks centroids for the street (falls back to `default_centroids`).
2. Computes `cluster_id` based on `board.len()`:
   - `0` (preflop): `flat_index_preflop(hole)` into the preflop table.
   - `3` (flop): `flat_index_flop(hole, board)` into the flop table — `combinadic_rank(5 sorted cards) × 10` + hole-mask index.
   - `4` (turn): `flat_index_turn(hole, board)` — `combinadic_rank_6(6 sorted cards) × 15` + hole-mask index.
   - `5` (river): `(hand_rank >> 6) << 8 | board_bucket` — `hand_rank >> 6` gives ~116 tiers from the 7462-cardinality raw rank; `board_bucket` comes from the river table.
3. Looks up `flop_bucket(board)` from the flop-buckets array.
4. Folds all of `street`, `history.len()`, `history`, `cluster_id`, `flop_bucket` into a `u64` via `fnv1a` starting from `FNV_OFFSET`.

The 10 hole-masks for flop and 15 for turn are statically tabulated inside `flat_index_*`. If a precomputed table isn't loaded (or the index falls outside its range), the abstraction falls back to `calculate_ehs` + `nearest_centroid` and emits a one-shot `eprintln!` warning via `warn_mc_fallback_once` — this path is ~100× slower per infoset than the table path.

### Combinadic helpers used

From `pkr_eval::lookup` and `pkr_eval::lookup_fast` (referenced but not in this snapshot):

- `choose(n, k) -> u32` — binomial coefficient.
- `combinadic_rank(cards) -> u32` — rank a sorted card slice.
- `combinadic_unrank_{2,3,5,6,7}(idx) -> [u8; N]` — unrank to a card array.
- A local `combinadic_rank_6([u8; 6]) -> u64` is defined in `pkr-abstraction/src/lib.rs`.

## Project Status

Verified from `CHANGELOG.md` (Unreleased section) and the actual code:

| Layer | State | Source |
|---|---|---|
| Workspace compiles with 11 crates | working | root `Cargo.toml` |
| `pkr-trainer` CLI orchestrates precompute → train → export | working | `binaries/pkr-trainer/src/main.rs` |
| `pkr-abstraction-precompute` binary with 12 subcommands | working | `crates/pkr-abstraction/src/bin/precompute.rs` |
| `KMeansAbstraction` with combinadic flat-indexing for all 4 streets | working | `crates/pkr-abstraction/src/lib.rs` |
| `calculate_ehs` Monte-Carlo equity with `EHS_SAMPLES` env override | working | `crates/pkr-abstraction/src/ehs.rs` |
| Atomic rolling checkpoint save (`.tmp → .prev → final`) | working | `save_checkpoint_rolling` in `main.rs` |
| `Trainer::with_capacity`, `load_checkpoint`, `save_checkpoint`, `iteration`, `get_table`, `run_iterations_parallel`, `is_near_capacity` | working | referenced from `main.rs` |
| `CompactRegretTable::with_capacity(n)` | working | `CHANGELOG.md` "Added" |
| `SolverHandle::debug_keys()` | working | `CHANGELOG.md` "Added" |
| `pkr-runtime` re-exports `SolverHandle` at crate root | working | `CHANGELOG.md` "Fixed" |
| `precompute` `hand_ranks` and `centroids` subcommands | working | `CHANGELOG.md` "Fixed" |
| `write_blueprint` sorts keys defensively and normalises CDF via `get_average_strategy_into` | working | `CHANGELOG.md` "Fixed" |
| `get_or_create_idx` CAS loop, clamp on save | working | `CHANGELOG.md` "Fixed" |
| `load_external_blueprint` ignored test in `pkr-trainer` | working | `CHANGELOG.md` "Added" |
| `pkr-exploit::best_response::sampled_exploitability` callable from CLI | working | referenced from `main.rs` |
| `pkr-fuzz` | scaffolded | root `Cargo.toml` members list (no source in snapshot) |

## Roadmap & unverified claims

The following claims appear in the project's existing `README.md` but are **not verifiable from the code in this snapshot** — the relevant source files (`pkr-cfr`, `pkr-export`, `pkr-runtime`, `pkr-eval`, `pkr-core`, `pkr-contracts`, `pkr-testgames`) were not included in the dump. They're listed here so a maintainer can either re-add them once the source is reviewed or update them if they've drifted.

- **Throughput numbers** — `~10,000 it/s` steady state (v36 measured 10,228 it/s, `v36-capacity-sweep.md`). Older text claimed `~27,000 it/s` (`15196 / 21159 / 27772 it/s` at iter 5120 / 51200 / 100000, `~2.3 billion iterations/day`) — **STALE**: that figure predates the F-series table changes and does not hold. Verifiable by running `./bench.sh` on M1, not by reading code.
- **CFR variant** — external-sampling MCCFR with DCFR discounting. Momentum (PCFR+) defaults OFF as of the F2 audit fix; set `PKR_MOMENTUM=1` to enable. Effective hyperparameters are read once via `pkr_cfr::config::TrainConfig` and recorded in `stats.json` under `env`.
- **Regret table internals** — `i32 regret + i64 strategy_sum at fixed-point scale 1000`, thread-local idx cache, "16 local iterations then merge + flush" inside the 256-iter batch. The `CompactRegretTable` source is not in this snapshot.
- **`blueprint.bin` layout** — `FileHeader` (32 bytes: magic, version, variant, count, k, hash_algo), `key_count: u32`, `cdf_size: u32`, `keys: u64 × key_count` (sorted), `cdf: u8 × cdf_size`. The `pkr-export/src/writer.rs` source is not in this snapshot.
- **Runtime API** — `MmapReader`, `SolverHandle`, `get_advice_fast(hash)`, "O(log n) binary search over sorted key array", "p99 < 1 ms". The `pkr-runtime` source is not in this snapshot (only `SolverHandle::debug_keys()` and the crate-root re-export are mentioned in `CHANGELOG.md`).
- **Working-set numbers** — `~600 MB` at 5M infosets, `>3 GB` at 50M, throughput degrades to `15-20K it/s` past that. Verifiable by running, not by reading code.
- **f32 saturation note** — `t^p + 1` rounds to `t^p` once `t^p` exceeds ~8.4e6, so the DCFR discount saturates to `1.0` and behaves like vanilla CFR. The `RatioPower` formula overflow around `t=3000` is also claimed. Both are attributed to `docs/status.md` in the existing README — `docs/` is not in this snapshot.
- **Test count** — `98 tests pass across 11 binaries, 1 ignored.` No test files in this snapshot; verifiable by running `cargo test`.
- **Kuhn poker harness** — `pkr-testgames` source not in this snapshot.

## Docs

Referenced from the existing `README.md` (the `docs/` directory is not in this snapshot, so these paths are unverified):

- `docs/INDEX.md` — full list with current/historical status
- `docs/status.md` — current state of the codebase and known issues
- `docs/arch-overview.md` — architecture and design decisions
- `docs/spec/architecture.md` — Level 3 architectural specification
- `docs/spec/bst.md` — Level 4 behavioral specifications and test plan
- `docs/pkr-sota-winning-roadmap.md` — roadmap
- `docs/tasks/` — machine-readable task definitions (historical)
- `docs/archive/` — superseded docs, kept for reference
