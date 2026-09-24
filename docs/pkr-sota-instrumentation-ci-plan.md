# pkr-sota — Instrumentation, Benchmarking & CI Integration Plan

> **Status:** ready to execute · **Audience:** an autonomous coding agent ("dumb agent") that executes tasks one at a time, in order · **Format:** self-contained task cards in the same style as `docs/tasks/done/W*-T*.md` and `docs/SOTA_UPGRADE_GUIDE_pkr-sota.md`
>
> **Source for this audit:** full read of `dump.txt` (15934 lines, 50 files, 11 crates + 1 binary).
>
> **Last updated:** 2026-09-25

---

## 0. How To Use This Document (read first — non-negotiable rules)

1. **Execute tasks in the order given by the priority table in §3.** Tasks inside the same tier may be executed in any order unless their "Dependencies" line says otherwise.
2. **Never modify** `crates/pkr-contracts/src/lib.rs` trait signatures, the workspace `Cargo.toml` `[workspace.dependencies]` block, the FNV-1a constants (`FNV_OFFSET`, `FNV_PRIME`), or `HASH_ALGO_FNV1A64_INFOSET`. Only this plan's task cards may touch them, and only when the card says so explicitly.
3. **Gates before declaring any task done** (every task, no exceptions):
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo nextest run --workspace --no-fail-fast
   ```
4. **After each tier**, run the end-to-end smoke: `./smoke.sh` must pass (including the byte-size checks for `turn_abstraction.bin` = `305377800` bytes and `river_buckets.bin` = `2598960` bytes, and the ignored `load_external_blueprint` test).
5. **One logical change per commit.** Commit message: `B<n>: <one-line summary>`. Never mix a refactor with a behavior change.
6. **Do not "improve" code not named in the task card.** If you find something broken outside the card's files, record it in `worklog.md` and move on.
7. **When a task card quotes "current code"**, match it against the real file first. If the real file differs (someone already changed it), STOP and record the mismatch in `worklog.md` before proceeding.
8. **Fast-iteration rule (the most important constraint in this plan):** the PR-gate workflow (`ci/fast.yml`) MUST complete in under 4 minutes wall-clock on a warm cache and under 8 minutes cold. Anything slower belongs in `ci/nightly.yml` or `ci/weekly.yml`. If you add a step that pushes fast.yml over 4 min, move it to nightly and note it in the worklog.
9. **Never use `println!`/`eprintln!` for benchmark output.** Always emit machine-readable files: JSON, CSV, or `cargo bench` Criterion's default NDJSON. The CI scripts parse these; human eyeballing is secondary.
10. **Every number written to disk must be accompanied by a unit and a timestamp.** No bare "15796.1" without a column header. The dashboards depend on this.

---

## 1. Where This Codebase Stands — Instrumentation Audit

### 1.1 What's already instrumented

| Layer | Where | What's measured | Format | CI integrated? |
|---|---|---|---|---|
| CFR training counters | `crates/pkr-cfr/src/metrics.rs` | nodes, depth, cache hits, infosets created, regret ops (in/out/dedup), strategy ops, traverse/merge/flush wall-ns, depth histogram | `LocalMetrics` + `GlobalMetrics` atomics, snapshot/delta | No |
| Trainer CLI reports | `binaries/pkr-trainer/src/main.rs` | per-window CSV: `iter,wall_s,it_per_s,infosets,...,cache_hit_rate,regret_dedup,...,traverse_ms,merge_ms,flush_ms,wall_ms` | `.proftest/metrics.csv`, `outputs/v1/metrics.csv` | No |
| Final-run snapshot | `binaries/pkr-trainer/src/main.rs` `stats_json` block | config, snapshot (infosets, capacity, regret stats, nonfinite count), cumulative metrics, strategy analysis (entropy hist, dominant counts), 200 sample infosets | `.proftest/stats.json` | No |
| Kuhn exploitability harness | `crates/pkr-testgames/src/bin/kuhn_experiment.rs` | 4 configs (vanilla / van-mom / canon / canon-mom), exploitability at 10 log-spaced checkpoints, max\|regret\|, NaN flag | stdout table | No |
| Sampled BR for NLHE | `crates/pkr-exploit/src/best_response.rs` | exploitability_mbb, br0, br1_to_p0, deals_sampled | `EVAL` line in trainer stderr | No |
| Thread-scaling benchmark | `bench.sh` | it/s for `THREADS_LIST=1 2 4 8`, `SECONDS_PER_RUN=15` each | stdout grep `BENCH` | No |
| Production profile | `proftest.sh` | full pipeline (precompute + 100K iter train + export + JSON validate) | `metrics.csv`, `stats.json`, `blueprint.bin` | No |
| Fast inner loop | `fast.sh` | cargo check + clippy (-D warnings) + nextest | exit code | No |
| End-to-end smoke | `smoke.sh` | 8 precompute steps + 10-iter train + export + runtime load test + byte-size asserts | exit code + `load_external_blueprint` ignored test | No |
| Justfile orchestration | `justfile` | aliases `fast`, `smoke`, `smoke-fresh`, `prof`, `bench`, `train`, `check` | n/a | No |

### 1.2 What's missing — and why each gap matters

| Gap | Cost of NOT closing | Where it would surface |
|---|---|---|
| **No CI workflow file.** Zero `.github/workflows/*.yml`, no `GitLab CI`, no `Drone`, no `Woodpecker`. | Every "did this break something?" question is answered by a human running `./fast.sh` locally. Regressions ship to `main` and are discovered weeks later when a smoke fails or throughput dips. | everywhere |
| **No `criterion` micro-bench suite.** `bench.sh` only measures full-trainer throughput; nothing isolates the hot inner functions (`fnv1a`, `evaluate_hand`, `get_advice_fast`, `get_infoset_hash`, `record_node`, regret apply). | A 30% regression in `get_advice_fast` is invisible because the dominant cost in `bench.sh` is traversal, not lookup. Runtime p99 target (`< 1 ms`) is unverifiable. | runtime, hash path |
| **No baseline storage.** `bench.sh` prints to stdout; nothing compares against last week's number. | "Is 27,700 it/s good or bad?" is unanswerable without history. Cannot tell signal from noise. | every bench run |
| **No memory/heap instrumentation.** `dhat`, `heaptrack`, `valgrind` not invoked anywhere. The 50M-capacity `~600 MB` working set is a number from `docs/README.md`, not a measured value. | Out-of-memory on the M1 will be diagnosed by guessing. Per-infoset memory cost (claim: 12 bytes) is unverified. | large training runs |
| **No binary-size tracking.** `release` profile uses `lto="fat"`, `codegen-units=1`, `strip="symbols"`. Output size is unmeasured. | Adding a dep that bloats `blueprint.bin` consumer binaries goes unnoticed. Runtime "small enough for VPS" claim is unverifiable. | release, runtime |
| **No compile-time tracking.** `lto="fat"` + `codegen-units=1` is notoriously slow. No measurement of how slow. | A 2-minute cold compile becomes 4 minutes and no one notices until dev loops die. | every developer |
| **No coverage report.** `cargo tarpaulin` / `cargo llvm-cov` not configured. | Tests pass but coverage could be 60% or 90%; no one knows. The "98 tests pass" stat is the only signal. | quality |
| **No fuzzing in CI.** `crates/pkr-fuzz` exists but is "unwired" per `docs/README.md`. No `cargo fuzz` targets run automatically. | Edge-case inputs (illegal action sequences, malformed blueprints, off-by-one board states) are uncovered until they hit production. | pkr-fuzz, pkr-runtime, pkr-export |
| **No `cargo audit` / `cargo deny` / `cargo outdated` / `cargo udeps`.** | A vulnerable `rand 0.10` or `dashmap 6` bump ships silently. Unused deps pile up. | supply chain |
| **No clippy trend.** Warnings-as-errors gates today's count but doesn't track new warnings added over time. | Code quality drifts; "fix this warning" becomes "fix these 47 warnings". | every PR |
| **No exploitability tracking over time.** Kuhn harness emits a table; nothing stores it or diffs against `main`. | A change that breaks convergence (see `docs/status.md` §"RatioPower → NaN at t=3000") ships green because exploitability is not a CI signal. | pkr-cfr, pkr-testgames, pkr-exploit |
| **No PR-comment diff.** A PR that regresses `it/s` by 5% gets merged because no one ran `bench.sh` against the branch. | Performance regressions are noticed post-merge, by which point the bisect is expensive. | every PR |
| **No flamegraph in CI.** `cargo flamegraph` requires local install. | Hot-path guesses ("is it the papaya map or the regret apply?") are unverifiable. | pkr-cfr |
| **No deadlock/panic sanitizer on the trainer.** `panic = "abort"` in release means a panic ends the run, but no CI asserts that no panics occurred across N iterations. | A latent bug in capacity clamping (already a fix in CHANGELOG) could re-emerge silently. | pkr-cfr table.rs |
| **No instrumentation of the runtime lookup path.** `SolverHandle::get_advice_fast` claims p99 < 1 ms; no CI micro-bench hits it. | "Sub-millisecond lookup" stays a marketing claim. | pkr-runtime |
| **No instrumentation of `pkr-export::write_blueprint`.** Export is in the smoke path but never timed in isolation. | A regression that doubles export time hides inside `proftest.sh` total wall. | pkr-export |
| **No tracking of MAPH size vs claim.** README claims "file size + ~10 MB" RSS. Unverified. | Runtime memory bloat (extra indices, cached decoded strategies) ships undetected. | pkr-runtime mmap.rs |

### 1.3 What's already in place that we should NOT replace

- The two-tier `LocalMetrics` + `GlobalMetrics` design in `crates/pkr-cfr/src/metrics.rs` is **correct**. Per-node writes are `&mut self` field increments (no atomics on the hot path), and `record_batch` is called once per `run_iterations_parallel`. Replacing this would slow training. We extend, not rewrite.
- The CSV column order in `binaries/pkr-trainer/src/main.rs` (line ~596) is **frozen** — `proftest.sh` and downstream analyzers depend on it. New columns append, never reorder.
- The Kuhn harness `kuhn_experiment.rs` already prints exploitability at log-spaced checkpoints. We parse its stdout and store the time series.
- `smoke.sh`'s byte-size asserts (`turn_abstraction.bin = 305377800`, `river_buckets.bin = 2598960`) are **regression gates** — they catch silent format changes. Keep them.
- The `fast.sh` three-step gate (`cargo check` → `cargo clippy -D warnings` → `cargo nextest --no-fail-fast`) is the right PR-gate core. We wrap, not replace.

---

## 2. Strategy — Tiered CI for Fast Iterations

### 2.1 Three tiers, three budgets

| Tier | Workflow | Trigger | Wall budget (cold) | What it runs | Failure action |
|---|---|---|---|---|---|
| **T1 Fast** | `.github/workflows/fast.yml` | every push, every PR | **< 4 min warm, < 8 min cold** | fmt check, clippy -D warnings, nextest --no-fail-fast, doctest, udeps, deny advisories | blocks merge |
| **T2 Smoke** | `.github/workflows/smoke.yml` | PR (when `smoke.sh` files or crate deps changed), nightly | < 12 min cold | full `smoke.sh` pipeline + JSON validate + byte-size asserts | blocks merge (if PR-triggered) |
| **T3 Bench** | `.github/workflows/bench.yml` | nightly @ 03:00 UTC, manual `workflow_dispatch` | ~30 min | criterion micro-bench suite (storage-tracked), thread-scaling bench (1/2/4/8 t), `proftest.sh` 100K-iter, Kuhn exploitability, binary size, compile time | posts comment to last PR if > 5% regression |
| **T4 Weekly** | `.github/workflows/weekly.yml` | every Mon 02:00 UTC | ~2 hr | full `proftest.sh` 1M-iter, memory profile via dhat, fuzz run 10 min, coverage report, deny license, audit, outdated, miri on `pkr-core` + `pkr-cfr` | opens issue with trend summary |

### 2.2 Why these budgets

- **T1 ≤ 4 min warm:** this is the dev-loop gate. If it exceeds 4 min, devs will run `--no-verify` and the gate is theater. The M1 dev machine does `./fast.sh` in "seconds when warm" per `fast.sh` comment; CI on a `ubuntu-22.04` runner with 4 vCPU is ~2.5× slower, so 4 min is the upper bound.
- **T2 ≤ 12 min cold:** the smoke pipeline builds 2 release binaries + precomputes 7 abstraction artifacts + trains 10 iters + exports + reloads. On M1 this is 2-5 min cold; on CI 4 vCPU we expect 8-11 min cold. If it exceeds 15 min we cache the abstraction artifacts.
- **T3 nightly ~30 min:** the bench tier runs ~40 micro-benchmarks (each 5-10 s sampling time) + 4 thread-scaling configs at 15 s each + a 100K-iter proftest (~3-5 min on 4 vCPU) + Kuhn (~1 min) + binary size (instant) + compile-time (instant). Total ≈ 25-30 min. This is non-blocking so it can run unattended.
- **T4 weekly ~2 hr:** 1M-iter proftest (~30 min on 4 vCPU), dhat memory profile (~10 min), 10-min fuzz run, coverage (~5 min), plus all T3 work. Scheduled for the weekend so it doesn't compete with PRs.

### 2.3 Storage and trending

We use **`bencher.dev` (self-hosted Bencher OSS, free for OSS projects)** OR **GitHub Actions cache + JSON artifacts committed to a `perf-history` branch** as the storage backend. The plan includes both paths; pick one based on whether you want a UI (Bencher) or zero external deps (branch history).

**Bencher.dev** (recommended):
- Free for public repos
- Tracks Criterion NDJSON output natively
- Generates PR comments with `+5.3% p<0.05` style diffs
- Has alerting (Slack/Discord/email) on threshold breach
- Setup: add `BENCHER_API_TOKEN` as a repo secret, install `bencher` CLI in the bench workflow

**Branch-history fallback** (zero external deps):
- Each nightly run commits `metrics/nightly/$(date -u +%Y-%m-%d)/bench.json` to a `perf-history` branch
- A `scripts/diff-perf.sh` reads the last two nightly JSONs and prints a Markdown table
- A `workflow_run` trigger posts the table as a comment on the most recent PR to `main`

The task cards in §5 are written to support both. The default is Bencher.

### 2.4 Quality gates — what blocks, what warns

| Metric | Gate (block) | Warn (comment, no block) | Source |
|---|---|---|---|
| Clippy warnings | any warning | n/a | `cargo clippy -- -D warnings` |
| Test pass rate | < 100% | n/a | `cargo nextest` |
| `smoke.sh` byte asserts | mismatch | n/a | `smoke.sh` |
| Micro-bench regression (criterion) | n/a | > 5% slower vs `main` (p < 0.05) | `bench.yml` + Bencher |
| Trainer throughput (proftest 100K) | n/a | it/s < 0.85 × baseline | `bench.yml` |
| Kuhn exploitability (t=1M) | n/a | exploitability > 1.5 × baseline | `bench.yml` |
| Binary size (`pkr-trainer`) | n/a | > 10% larger vs `main` | `bench.yml` |
| Compile time (cold, `--profile=release`) | n/a | > 20% slower vs `main` | `bench.yml` |
| `cargo audit` | any RUSTSEC with severity ≥ medium | any RUSTSEC low | `weekly.yml` |
| Coverage (weekly) | n/a | drops > 5 pp vs last week | `weekly.yml` |
| Memory RSS at 100K iters | n/a | > 1.5 × baseline | `weekly.yml` |
| Fuzz crash | any crash | n/a | `weekly.yml` |

**Rationale for "warn, don't block" on perf:** a perf regression that's < 5% is in the noise floor for most workloads and blocking on it kills iteration speed. But the PR author MUST see the number and consciously decide. Bencher's PR comment does this; if they ignore it, the weekly trend catches systemic drift.

**Rationale for "block" on clippy/tests/smoke:** these are objective. A test failure is never noise. A clippy warning never goes away. A byte-size mismatch means the file format changed and that's always a deliberate decision.

---

## 3. Task Index — Priority Order

| ID | Tier | Title | Impact | Effort | Risk | Block fast.yml? |
|----|------|-------|--------|--------|------|------------------|
| B1 | 0 | Bootstrap CI directory + `fast.yml` PR gate | Very high | S | Low | n/a (it IS the gate) |
| B2 | 0 | `smoke.yml` — CI version of `smoke.sh` | Very high | S | Low | no |
| B3 | 0 | `audit.yml` — daily `cargo audit` + `cargo deny` | High | S | Low | no |
| B4 | 1 | Add `criterion` dev-dep + `benches/` directory skeleton | High | S | Low | no |
| B5 | 1 | Micro-benches: `pkr-contracts::fnv1a` + `pkr-core` hot fns | High | S | Low | no |
| B6 | 1 | Micro-benches: `pkr-eval::TableEvaluator` + `slow` parity | High | M | Low | no |
| B7 | 1 | Micro-benches: `pkr-runtime::SolverHandle::get_advice_fast` | Very high | S | Low | no |
| B8 | 1 | Micro-benches: `pkr-cfr::CompactRegretTable` ops | High | M | Low | no |
| B9 | 1 | Micro-benches: `pkr-abstraction::get_infoset_hash` | High | S | Low | no |
| B10 | 1 | `bench.yml` nightly — wraps criterion + thread-scaling | Very high | M | Low | no |
| B11 | 1 | Wire Bencher (or branch-history fallback) for trending | Very high | M | Low | no |
| B12 | 2 | `proftest-ci.yml` — production-scale profile in CI | High | M | Medium | no |
| B13 | 2 | Parse `metrics.csv` + `stats.json` into Bencher custom metrics | High | M | Low | no |
| B14 | 2 | Kuhn exploitability tracked as a CI metric | High | S | Low | no |
| B15 | 2 | Sampled-BR exploitability tracked for NLHE | High | M | Low | no |
| B16 | 2 | Binary-size + compile-time tracking | Medium | S | Low | no |
| B17 | 2 | Coverage via `cargo-llvm-cov` in `weekly.yml` | Medium | S | Low | no |
| B18 | 3 | Memory profile with `dhat` in `weekly.yml` | Medium | M | Medium | no |
| B19 | 3 | Fuzz run wired into `weekly.yml` (10 min budget) | High | M | Medium | no |
| B20 | 3 | `miri` run on `pkr-core` + `pkr-cfr` (subset) | Medium | M | Medium | no |
| B21 | 3 | PR-comment diff bot (criterion vs main) | Very high | S | Low | no |
| B22 | 3 | Dashboard README badge row (status + trend) | Low | S | Low | no |
| B23 | 4 | Stretch: flamegraph on proftest, committed as artifact | Research | L | Medium | no |

**Execution order note:** **B1 before anything** (without a CI runner, nothing else triggers), **B4 before B5-B9** (criterion must exist before benches are added), **B10 before B11** (need bench output before storing it), **B12 before B13** (need proftest output before parsing it), **B14 paired with T3 from `SOTA_UPGRADE_GUIDE`** (Kuhn BR math is the same math), **B21 last in tier 3** (needs B11 + B10 to have produced a baseline).

---

## 4. File Inventory — What Will Exist After This Plan

```
.
├── .github/
│   └── workflows/
│       ├── fast.yml            (B1)  — PR gate, < 4 min
│       ├── smoke.yml           (B2)  — pipeline gate, < 12 min
│       ├── audit.yml           (B3)  — daily supply-chain
│       ├── bench.yml           (B10) — nightly micro-bench + thread-scaling
│       ├── proftest-ci.yml     (B12) — nightly 100K-iter proftest
│       ├── weekly.yml          (B17/B18/B19/B20) — weekly deep
│       └── label-pr.yml        (B21) — auto-label perf-impact PRs
├── benches/
│   ├── workspace-bench.toml    (B4)  — shared criterion config
│   ├── pkr_contracts_bench/
│   │   ├── Cargo.toml
│   │   └── benches/fnv1a.rs   (B5)
│   ├── pkr_core_bench/
│   │   ├── Cargo.toml
│   │   └── benches/{card,deck,state}.rs (B5)
│   ├── pkr_eval_bench/
│   │   ├── Cargo.toml
│   │   └── benches/{table,slow,parity}.rs (B6)
│   ├── pkr_runtime_bench/
│   │   ├── Cargo.toml
│   │   └── benches/lookup.rs   (B7)
│   ├── pkr_cfr_bench/
│   │   ├── Cargo.toml
│   │   └── benches/{table,dcfr,metrics}.rs (B8)
│   └── pkr_abstraction_bench/
│       ├── Cargo.toml
│       └── benches/abstraction.rs (B9)
├── ci/
│   ├── scripts/
│   │   ├── run-fast.sh          (B1)  — wrapper invoked by fast.yml
│   │   ├── run-smoke.sh         (B2)  — wrapper invoked by smoke.yml
│   │   ├── run-bench.sh         (B10) — wrapper for criterion + scaling
│   │   ├── run-proftest-ci.sh   (B12) — wrapper for proftest in CI
│   │   ├── parse-metrics-csv.py (B13) — metrics.csv → Bencher custom
│   │   ├── parse-stats-json.py  (B13) — stats.json → Bencher custom
│   │   ├── parse-kuhn.py        (B14) — kuhn_experiment stdout → JSON
│   │   ├── measure-binary-size.sh (B16)
│   │   ├── measure-compile-time.sh (B16)
│   │   ├── diff-perf.sh         (B11 fallback) — last 2 nightly JSONs
│   │   └── post-pr-comment.sh   (B21) — PR comment with diff table
│   └── bencher.yml              (B11) — Bencher project config
├── .cargo/
│   └── config.toml               (B16) — pinned lld/mold, target-cpu
├── Cargo.toml                    (modified: add [workspace.dev-dependencies] criterion)
└── README.md                     (modified: add CI badges, B22)
```

**Total: 23 new files + 4 modified files. No existing source file in `crates/` is touched by B1-B22 except to add `#[bench]`-style imports where the bench lives in-tree (we use the external `benches/` directory pattern instead, so in-tree files stay clean).**

---

# TIER 0 — Bootstrap CI

## B1 — Bootstrap CI directory + `fast.yml` PR gate

**Objective.** There is currently zero CI. The first thing a dumb agent must do is create the workflow directory and a single `fast.yml` that wraps `fast.sh`. This becomes the gate every subsequent task card is verified against.

**Exclusive File Paths**
- `.github/workflows/fast.yml` (new)
- `ci/scripts/run-fast.sh` (new)

**Dependencies**
- A GitHub repo (push access). If you are on GitLab/Drone, adapt the YAML syntax but keep the step ordering.

**Instructions**

1. Create the directory structure:
   ```bash
   mkdir -p .github/workflows ci/scripts
   ```

2. Write `ci/scripts/run-fast.sh`:
   ```bash
   #!/usr/bin/env bash
   # CI wrapper for ./fast.sh. Used by .github/workflows/fast.yml.
   # Runs fmt-check, clippy -D warnings, nextest --no-fail-fast, doctest.
   # Exits non-zero on any failure. Logs are kept raw for the GitHub UI.
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   echo "==> [1/4] cargo fmt --check"
   cargo fmt --all -- --check

   echo "==> [2/4] cargo clippy (workspace, -D warnings)"
   cargo clippy --workspace --all-targets --quiet -- -D warnings

   echo "==> [3/4] cargo nextest (workspace, --no-fail-fast)"
   cargo nextest run --workspace --no-fail-fast

   echo "==> [4/4] cargo test --doc"
   cargo test --doc --workspace --quiet

   echo "=== run-fast.sh passed ==="
   ```

3. Make it executable:
   ```bash
   chmod +x ci/scripts/run-fast.sh
   ```

4. Write `.github/workflows/fast.yml`:
   ```yaml
   name: fast

   on:
     push:
       branches: [main, master]
     pull_request:

   # Cancel in-progress runs on the same ref — keeps fast iterations cheap.
   concurrency:
     group: fast-${{ github.ref }}
     cancel-in-progress: true

   jobs:
     fast:
       runs-on: ubuntu-22.04
       timeout-minutes: 10
       steps:
         - uses: actions/checkout@v4
           with:
             # Full history for cargo cache key
             fetch-depth: 1

         - name: Install Rust toolchain (stable, minimal)
           uses: dtolnay/rust-toolchain@stable
           with:
             components: clippy, rustfmt

         - name: Install cargo-nextest
           uses: taiki-e/install-action@v2
           with:
             tool: cargo-nextest

         - name: Cache cargo registry + target
           uses: Swatinem/rust-cache@v2
           with:
             shared-key: fast-release
             cache-targets: true
             cache-on-failure: true

         - name: Run fast gate
           run: ./ci/scripts/run-fast.sh
   ```

5. Commit and push:
   ```bash
   git add .github/workflows/fast.yml ci/scripts/run-fast.sh
   git commit -m "B1: add fast.yml PR gate (fmt + clippy + nextest + doctest)"
   git push
   ```

6. Open a PR. Confirm the `fast` workflow runs green. If it fails, fix the underlying issue (do NOT relax the gate; that defeats the purpose).

**Acceptance Criteria**
- [ ] `.github/workflows/fast.yml` exists and triggers on push and PR.
- [ ] `ci/scripts/run-fast.sh` is executable and exits 0 on a clean tree.
- [ ] Cold wall time on `ubuntu-22.04` is < 8 min. Warm wall time (cache hit) is < 4 min.
- [ ] `concurrency.cancel-in-progress: true` is set so pushing twice doesn't queue.
- [ ] PR merge is blocked (GitHub branch protection) unless `fast` is green.

---

## B2 — `smoke.yml` — CI version of `smoke.sh`

**Objective.** `smoke.sh` runs the full 8-step pipeline (precompute → train 10 iters → export → reload). It must run on every PR that touches anything other than docs/markdown, and nightly as a baseline.

**Exclusive File Paths**
- `.github/workflows/smoke.yml` (new)
- `ci/scripts/run-smoke.sh` (new)
- `ci/cache-key.sh` (new — abstraction artifact cache key)

**Dependencies**
- B1 (the runner env exists).

**Instructions**

1. Write `ci/cache-key.sh`:
   ```bash
   #!/usr/bin/env bash
   # Emits a cache key for the .smoke/ abstraction artifacts.
   # Key = hash of every file that influences abstraction output.
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   FILES=(
     Cargo.toml
     crates/pkr-abstraction/Cargo.toml
     crates/pkr-abstraction/src/bin/precompute.rs
     crates/pkr-abstraction/src/ehs.rs
     crates/pkr-abstraction/src/lib.rs
     crates/pkr-core/src/card.rs
     crates/pkr-core/src/deck.rs
     crates/pkr-core/src/rules.rs
     crates/pkr-core/src/state.rs
     crates/pkr-eval/src/lib.rs
     crates/pkr-eval/src/lookup.rs
     crates/pkr-eval/src/lookup_fast.rs
     crates/pkr-eval/src/slow.rs
     smoke.sh
   )
   cat "${FILES[@]}" | sha256sum | cut -d' ' -f1
   ```

2. Write `ci/scripts/run-smoke.sh`:
   ```bash
   #!/usr/bin/env bash
   # CI wrapper for ./smoke.sh. Restores/saves the .smoke/ cache so the
   # 8-step pipeline only runs the train+export+reload tail on each PR.
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   # The smoke script writes to ./outputs/v0-smoke by default.
   export SMOKE_DIR="${SMOKE_DIR:-./outputs/v0-smoke}"
   mkdir -p "$SMOKE_DIR"

   # Run smoke. The script's own `ensure` function skips artifacts that
   # already exist on disk; the cache restore in the YAML populates them.
   ./smoke.sh

   # Belt-and-suspenders: re-validate the byte-size asserts that smoke.sh
   # already checks. This catches silent format regressions even if
   # someone removes them from smoke.sh.
   python3 - <<'PY'
   import os, sys
   d = os.environ["SMOKE_DIR"]
   checks = {
       "turn_abstraction.bin": 305377800,
       "river_buckets.bin":    2598960,
   }
   for name, expected in checks.items():
       p = os.path.join(d, name)
       if not os.path.exists(p):
           print(f"FAIL: {p} missing", file=sys.stderr); sys.exit(1)
       actual = os.path.getsize(p)
       if actual != expected:
           print(f"FAIL: {name} size {actual} != {expected}", file=sys.stderr)
           sys.exit(1)
       print(f"OK: {name} = {actual} bytes")
   PY

   # Run the load test that smoke.sh runs at the end, in-process, so
   # we get a JUnit-style failure if it breaks.
   PKR_BLUEPRINT="$SMOKE_DIR/blueprint.bin" \
       cargo test --release -p pkr-trainer --test pipeline -- --ignored load_external_blueprint
   ```

3. Make both scripts executable:
   ```bash
   chmod +x ci/scripts/run-smoke.sh ci/cache-key.sh
   ```

4. Write `.github/workflows/smoke.yml`:
   ```yaml
   name: smoke

   on:
     pull_request:
       paths:
         - 'crates/**'
         - 'binaries/**'
         - 'Cargo.toml'
         - 'smoke.sh'
         - 'ci/**'
         - '.github/workflows/smoke.yml'
     schedule:
       - cron: '17 4 * * *'   # nightly 04:17 UTC
   workflow_dispatch:

   concurrency:
     group: smoke-${{ github.ref }}
     cancel-in-progress: true

   jobs:
     smoke:
       runs-on: ubuntu-22.04
       timeout-minutes: 20
       steps:
         - uses: actions/checkout@v4

         - uses: dtolnay/rust-toolchain@stable

         - name: Cache smoke abstraction artifacts
           id: cache-smoke
           uses: actions/cache@v4
           with:
             path: outputs/v0-smoke
             key: smoke-abstractions-${{ runner.os }}-${{ hashFiles('ci/cache-key.sh') }}-${{ hashFiles('ci/cache-key.sh', 'smoke.sh') }}
             restore-keys: |
               smoke-abstractions-${{ runner.os }}-

         - name: Compute cache key
           id: key
           run: echo "k=$(./ci/cache-key.sh)" >> $GITHUB_OUTPUT

         - name: Cache smoke (refined)
           uses: actions/cache@v4
           with:
             path: outputs/v0-smoke
             key: smoke-abstractions-${{ runner.os }}-${{ steps.key.outputs.k }}

         - name: Cache cargo
           uses: Swatinem/rust-cache@v2
           with:
             shared-key: smoke-release
             cache-targets: true

         - name: Run smoke
           env:
             SMOKE_DIR: outputs/v0-smoke
           run: ./ci/scripts/run-smoke.sh

         - name: Upload artifacts on failure
           if: failure()
           uses: actions/upload-artifact@v4
           with:
             name: smoke-fail
             path: |
              outputs/v0-smoke/metrics.csv
              outputs/v0-smoke/stats.json
              outputs/v0-smoke/blueprint.bin
             retention-days: 7
   ```

5. Commit, push, open PR.

**Acceptance Criteria**
- [ ] `smoke.yml` triggers on PRs that touch `crates/`, `binaries/`, `Cargo.toml`, `smoke.sh`, or `ci/`.
- [ ] Nightly run at 04:17 UTC keeps a baseline.
- [ ] Cache hit on warm runs (abstraction artifacts restored) cuts wall time below the cold budget.
- [ ] Byte-size asserts in `ci/scripts/run-smoke.sh` are independent of `smoke.sh` so removing them from `smoke.sh` doesn't disable the gate.
- [ ] On failure, artifacts upload for offline debugging.

---

## B3 — `audit.yml` — daily supply-chain check

**Objective.** A vulnerable `rand` or `dashmap` ships silently today. `cargo audit` (RUSTSEC) + `cargo deny` (license + advisories + banned deps) catches this. Daily run, blocking for medium+ RUSTSEC.

**Exclusive File Paths**
- `.github/workflows/audit.yml` (new)
- `deny.toml` (new — at repo root)
- `ci/scripts/run-audit.sh` (new)

**Dependencies**
- B1.

**Instructions**

1. Write `deny.toml`:
   ```toml
   # cargo-deny configuration. See https://embarkstudios.github.io/cargo-deny/.
   [graph]
   # No features gating — analyze the actual graph as built.
   all-features = true

   [advisories]
   # Treat RUSTSEC entries with severity >= "medium" as CI-fail.
   # Low-severity items are reported but do not block.
   version = 2
   ignore = [
     # Add RUSTSEC-IDs here ONLY with a comment explaining why and
     # when the ignore can be removed. Example:
     # { id = "RUSTSEC-2024-0000", reason = "dev-only dep, no fix upstream", expires = "2026-12-01" },
   ]

   [licenses]
   # Allow common permissive licenses. Anything else needs human review.
   allow = [
     "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause",
     "ISC", "Unicode-DFS-2016", "Zlib", "CC0-1.0",
   ]
   confidence-threshold = 0.8

   [bans]
   # Fail if multiple versions of the same crate are pulled in.
   multiple-versions = "warn"
   # Fail on wildcard deps (e.g., "regex = *").
   wildcards = "deny"
   highlight = "all"

   [sources]
   # Only crates-io and our own workspace paths are allowed.
   unknown-registry = "deny"
   unknown-git = "deny"
   allow-registry = ["https://github.com/rust-lang/crates.io-index"]
   ```

2. Write `ci/scripts/run-audit.sh`:
   ```bash
   #!/usr/bin/env bash
   # Daily supply-chain gate. Exits non-zero on:
   #  - any RUSTSEC advisory with severity >= medium
   #  - any license violation per deny.toml
   #  - any wildcard dependency
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   echo "==> cargo audit"
   cargo audit --deny warnings

   echo "==> cargo deny check"
   cargo deny check advisories bans licenses sources
   ```

3. Write `.github/workflows/audit.yml`:
   ```yaml
   name: audit

   on:
     schedule:
       - cron: '13 5 * * *'   # daily 05:13 UTC
     pull_request:
       paths:
         - 'Cargo.toml'
         - 'Cargo.lock'
         - 'deny.toml'
   workflow_dispatch:

   jobs:
     audit:
       runs-on: ubuntu-22.04
       timeout-minutes: 5
       steps:
         - uses: actions/checkout@v4

         - uses: dtolnay/rust-toolchain@stable

         - name: Install cargo-audit + cargo-deny
           uses: taiki-e/install-action@v2
           with:
             tool: cargo-audit,cargo-deny

         - name: Run audit
           run: ./ci/scripts/run-audit.sh
   ```

4. Commit, push.

**Acceptance Criteria**
- [ ] `deny.toml` exists with allow-list covering all current licenses.
- [ ] `cargo audit --deny warnings` exits 0 on current `Cargo.lock`.
- [ ] `cargo deny check advisories bans licenses sources` exits 0 on current tree.
- [ ] Workflow triggers daily and on `Cargo.toml`/`Cargo.lock`/`deny.toml` PRs.
- [ ] Any future RUSTSEC with severity ≥ medium blocks the workflow.

---

# TIER 1 — Micro-benchmark suite

## B4 — Add `criterion` dev-dep + `benches/` skeleton

**Objective.** Every micro-bench in this plan uses `criterion` for noise-aware statistical comparison. We add criterion as a workspace dev-dependency once, and lay down a `benches/<crate>_bench/` directory per crate that will have benches.

**Exclusive File Paths**
- `Cargo.toml` (modified — add `[workspace.dev-dependencies]` block)
- `benches/workspace-bench.toml` (new — shared criterion config)

**Dependencies**
- B1.

**Instructions**

1. In the workspace root `Cargo.toml`, add a `[workspace.dev-dependencies]` block right after `[workspace.dependencies]`:
   ```toml
   [workspace.dev-dependencies]
   criterion = { version = "0.5", features = ["html_reports", "cargo_bench_support"] }
   ```

2. Write `benches/workspace-bench.toml` (criterion will read this from each bench crate's Cargo.toml; we duplicate it to keep a single source of truth):
   ```toml
   # Shared criterion config. Each benches/<crate>_bench/Cargo.toml
   # references this via [[bench]] metadata.
   #
   # Tuned for CI: short warm-up, modest sample size, no html_reports
   # (we emit NDJSON for Bencher instead).
   [criterion]
   warm_up_time = 2        # seconds (default 3; we cut for CI budget)
   measurement_time = 5   # seconds per benchmark (default 5)
   sample_size = 30        # default 100; we cut to 30 for CI noise tolerance
   quiet = true

   [criterion.benchmarks]
   # Empty — per-bench tuning lives in the bench source.
   ```

3. The directory `benches/` is created empty in this task; B5-B9 populate it.

4. Add `benches/` to `.gitignore`? **No** — these are committed source files. Add `benches/*/target/` and `benches/*/Cargo.lock` to `.gitignore` instead.

   Append to `.gitignore`:
   ```gitignore
   # Bench crate build artifacts (do not commit; reused via cargo workspace)
   benches/*/target
   benches/*/Cargo.lock
   # Criterion HTML reports
   benches/*/target/criterion
   ```

5. Commit:
   ```bash
   git add Cargo.toml benches/workspace-bench.toml .gitignore
   git commit -m "B4: add criterion workspace dev-dep + benches/ skeleton"
   ```

**Acceptance Criteria**
- [ ] `cargo build --workspace` still passes.
- [ ] `criterion` 0.5 is resolvable: `cargo tree -e dev | grep criterion`.
- [ ] `benches/workspace-bench.toml` exists with the CI-tuned parameters.
- [ ] `.gitignore` ignores bench build artifacts but not the bench sources.

---

## B5 — Micro-benches: `pkr-contracts::fnv1a` + `pkr-core` hot fns

**Objective.** `fnv1a` is called once per infoset hash, on every CFR node visited. Per `docs/status.md`, training does ~280 nodes/iter × ~27K it/s = ~7.5M fnv1a calls/sec. A 50% regression here is invisible in `bench.sh` but costs 4% of throughput. We also bench `Card::new`, `Deck::shuffle` (well, the deal path), and `GameState` transitions.

**Exclusive File Paths**
- `benches/pkr_contracts_bench/Cargo.toml` (new)
- `benches/pkr_contracts_bench/benches/fnv1a.rs` (new)
- `benches/pkr_core_bench/Cargo.toml` (new)
- `benches/pkr_core_bench/benches/card.rs` (new)
- `benches/pkr_core_bench/benches/deck.rs` (new)
- `benches/pkr_core_bench/benches/state.rs` (new)

**Dependencies**
- B4.

**Instructions**

1. Create the bench-crate directories:
   ```bash
   mkdir -p benches/pkr_contracts_bench/benches
   mkdir -p benches/pkr_core_bench/benches
   ```

2. Write `benches/pkr_contracts_bench/Cargo.toml`:
   ```toml
   [package]
   name = "pkr-contracts-bench"
   version = "0.0.0"
   edition = "2021"
   publish = false

   [dependencies]
   pkr-contracts = { workspace = true }

   [dev-dependencies]
   criterion = { workspace = true }

   [[bench]]
   name = "fnv1a"
   harness = false
   ```

3. Write `benches/pkr_contracts_bench/benches/fnv1a.rs`:
   ```rust
   //! Benchmark FNV-1a 64-bit. This is called on every infoset hash,
   //! so ~7.5M times/sec during training. A 50 ns regression here costs
   //! ~375 ms/sec of throughput.
   //!
   //! Run: cargo bench -p pkr-contracts-bench --bench fnv1a

   use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
   use pkr_contracts::{fnv1a, FNV_OFFSET};

   fn bench_fnv1a_small(c: &mut Criterion) {
       // 8-byte input — typical for a single u64 history byte slice.
       let input = b"abcdefgh";
       c.bench_function("fnv1a/u64_input", |b| {
           b.iter(|| {
               let mut h = FNV_OFFSET;
               fnv1a(&mut h, black_box(input));
               black_box(h)
           })
       });
   }

   fn bench_fnv1a_varying(c: &mut Criterion) {
       // Real infoset hashes mix: 2-byte hole + 5-byte board + N-byte history.
       // Sizes: preflop = 2+0+~8, flop = 2+3+~8, turn = 2+4+~8, river = 2+5+~8.
       let sizes: &[(usize, &str)] = &[
           (10, "preflop"),
           (13, "flop"),
           (14, "turn"),
           (15, "river"),
       ];
       let mut group = c.benchmark_group("fnv1a/by_street");
       for &(n, label) in sizes {
           let input: Vec<u8> = (0..n).map(|i| i as u8).collect();
           group.bench_with_input(BenchmarkId::from_parameter(label), &input, |b, data| {
               b.iter(|| {
                   let mut h = FNV_OFFSET;
                   fnv1a(&mut h, black_box(data));
                   black_box(h)
               })
           });
       }
       group.finish();
   }

   criterion_group!(benches, bench_fnv1a_small, bench_fnv1a_varying);
   criterion_main!(benches);
   ```

4. Write `benches/pkr_core_bench/Cargo.toml`:
   ```toml
   [package]
   name = "pkr-core-bench"
   version = "0.0.0"
   edition = "2021"
   publish = false

   [dependencies]
   pkr-core = { workspace = true }
   pkr-contracts = { workspace = true }

   [dev-dependencies]
   criterion = { workspace = true }

   [[bench]]
   name = "card"
   harness = false

   [[bench]]
   name = "deck"
   harness = false

   [[bench]]
   name = "state"
   harness = false
   ```

5. Write `benches/pkr_core_bench/benches/card.rs`:
   ```rust
   //! Benchmark Card::new and Card::from_u8 (if present).
   //! These are called millions of times per training second.

   use criterion::{black_box, criterion_group, criterion_main, Criterion};
   use pkr_core::card::{Card, Rank, Suit};

   fn bench_card_new(c: &mut Criterion) {
       c.bench_function("card/new", |b| {
           b.iter(|| {
               for r in 0..13u8 {
                   for s in 0..4u8 {
                       let rank = unsafe { std::mem::transmute::<u8, Rank>(r) };
                       let suit = unsafe { std::mem::transmute::<u8, Suit>(s) };
                       black_box(Card::new(suit, rank));
                   }
               }
           })
       });
   }

   criterion_group!(benches, bench_card_new);
   criterion_main!(benches);
   ```

6. Write `benches/pkr_core_bench/benches/deck.rs`:
   ```rust
   //! Benchmark Deck::deal path (the part called per-traverse).

   use criterion::{black_box, criterion_group, criterion_main, Criterion};

   // Mirror the exact API pkr_core exposes. If the function names differ,
   // match the real file (rule 7).
   use pkr_core::deck::Deck;

   fn bench_deck_deal_5(c: &mut Criterion) {
       c.bench_function("deck/deal_5", |b| {
           b.iter_with_setup(
               || Deck::new(),
               |mut deck| {
                   let mut buf = [0u8; 5];
                   for i in 0..5 {
                       buf[i] = black_box(deck.deal_one());
                   }
                   buf
               },
               |_| (),
           )
       });
   }

   criterion_group!(benches, bench_deck_deal_5);
   criterion_main!(benches);
   ```

7. Write `benches/pkr_core_bench/benches/state.rs`:
   ```rust
   //! Benchmark GameState::legal_actions_into — the per-node allocator.

   use criterion::{black_box, criterion_group, criterion_main, Criterion};
   use pkr_core::state::GameState;

   fn bench_legal_actions(c: &mut Criterion) {
       let mut state = GameState::new_heads_up();
       let mut buf = [0u8; 16];
       c.bench_function("state/legal_actions_into", |b| {
           b.iter(|| {
               let n = state.legal_actions_into(&mut black_box(buf));
               black_box(n)
           })
       });
   }

   criterion_group!(benches, bench_legal_actions);
   criterion_main!(benches);
   ```

8. **Match-against-real-file check (rule 7):** open each of `crates/pkr-core/src/card.rs`, `deck.rs`, `state.rs` and `crates/pkr-contracts/src/lib.rs`. Confirm the function names `Card::new`, `Deck::new`, `Deck::deal_one`, `GameState::new_heads_up`, `GameState::legal_actions_into` exist with those signatures. If any differs, replace the bench call site with the real signature and note the mismatch in `worklog.md`.

9. Commit:
   ```bash
   git add benches/
   git commit -m "B5: add pkr-contracts (fnv1a) + pkr-core (card, deck, state) micro-benches"
   ```

10. Run locally to confirm they work:
    ```bash
    cargo bench -p pkr-contracts-bench --bench fnv1a
    cargo bench -p pkr-core-bench --bench card
    cargo bench -p pkr-core-bench --bench deck
    cargo bench -p pkr-core-bench --bench state
    ```

**Acceptance Criteria**
- [ ] `cargo bench -p pkr-contracts-bench` produces criterion output with at least 3 bench cases (1 small + 4 streets, minus grouping).
- [ ] `cargo bench -p pkr-core-bench` produces output for card, deck, and state.
- [ ] No compile errors after matching real signatures.
- [ ] `benches/workspace-bench.toml`'s reduced sample size (30) is honored (visible in `--verbose` output).

---

## B6 — Micro-benches: `pkr-eval::TableEvaluator` + parity check vs `slow`

**Objective.** `TableEvaluator` is the per-node hand evaluator. `bench.sh` shows training does ~280 nodes/iter × ~27K it/s = ~7.5M evals/sec. Any regression here is the highest-impact single regression in the codebase. We also bench `slow::NlheEvaluator` to keep the "fast / slow parity" property tight.

**Exclusive File Paths**
- `benches/pkr_eval_bench/Cargo.toml` (new)
- `benches/pkr_eval_bench/benches/table.rs` (new)
- `benches/pkr_eval_bench/benches/slow.rs` (new)
- `benches/pkr_eval_bench/benches/parity.rs` (new — asserts slow == table on the same inputs)

**Dependencies**
- B4, B5.

**Instructions**

1. `mkdir -p benches/pkr_eval_bench/benches`

2. Write `benches/pkr_eval_bench/Cargo.toml`:
   ```toml
   [package]
   name = "pkr-eval-bench"
   version = "0.0.0"
   edition = "2021"
   publish = false

   [dependencies]
   pkr-eval = { workspace = true }
   pkr-core = { workspace = true }

   [dev-dependencies]
   criterion = { workspace = true }

   [[bench]]
   name = "table"
   harness = false

   [[bench]]
   name = "slow"
   harness = false

   [[bench]]
   name = "parity"
   harness = true   # uses default #[test] harness
   ```

3. Write `benches/pkr_eval_bench/benches/table.rs`:
   ```rust
   //! Benchmark TableEvaluator::evaluate_hand.
   //!
   //! Requires a hand_ranks.bin — point PKR_HAND_RANKS at it.

   use criterion::{black_box, criterion_group, criterion_main, Criterion};
   use pkr_core::card::{Card, Rank, Suit};
   use pkr_eval::{Evaluator, TableEvaluator};
   use std::env;

   fn make_evaluator() -> TableEvaluator {
       let path = env::var("PKR_HAND_RANKS")
           .expect("PKR_HAND_RANKS must point at hand_ranks.bin");
       TableEvaluator::new(&path).expect("failed to load hand_ranks.bin")
   }

   fn sample_hole() -> [Card; 2] {
       [
           Card::new(Suit::Spade, Rank::Ace),
           Card::new(Suit::Heart, Rank::King),
       ]
   }

   fn sample_board() -> [Card; 5] {
       [
           Card::new(Suit::Diamond, Rank::Two),
           Card::new(Suit::Club,    Rank::Three),
           Card::new(Suit::Spade,   Rank::Four),
           Card::new(Suit::Heart,   Rank::Five),
           Card::new(Suit::Diamond, Rank::Six),
       ]
   }

   fn bench_evaluate_preflop(c: &mut Criterion) {
       let e = make_evaluator();
       let hole: Vec<u8> = sample_hole().iter().map(|c| c.to_u8()).collect();
       let board: Vec<u8> = vec![];  // preflop
       c.bench_function("table_eval/preflop", |b| {
           b.iter(|| black_box(e.evaluate_hand(black_box(&hole), black_box(&board))))
       });
   }

   fn bench_evaluate_river(c: &mut Criterion) {
       let e = make_evaluator();
       let hole: Vec<u8> = sample_hole().iter().map(|c| c.to_u8()).collect();
       let board: Vec<u8> = sample_board().iter().map(|c| c.to_u8()).collect();
       c.bench_function("table_eval/river", |b| {
           b.iter(|| black_box(e.evaluate_hand(black_box(&hole), black_box(&board))))
       });
   }

   criterion_group!(benches, bench_evaluate_preflop, bench_evaluate_river);
   criterion_main!(benches);
   ```

4. Write `benches/pkr_eval_bench/benches/slow.rs` (same shape, but using `pkr_eval::NlheEvaluator` from `crates/pkr-eval/src/slow.rs`):
   ```rust
   //! Benchmark the slow path (no lookup tables). This is the parity
   //! reference for the fast path; it must not silently diverge.

   use criterion::{black_box, criterion_group, criterion_main, Criterion};
   use pkr_core::card::{Card, Rank, Suit};
   use pkr_eval::slow::NlheEvaluator;

   fn make_evaluator() -> NlheEvaluator {
       NlheEvaluator::new()
   }

   fn sample_hole() -> [Card; 2] {
       [
           Card::new(Suit::Spade, Rank::Ace),
           Card::new(Suit::Heart, Rank::King),
       ]
   }

   fn sample_board() -> [Card; 5] {
       [
           Card::new(Suit::Diamond, Rank::Two),
           Card::new(Suit::Club,    Rank::Three),
           Card::new(Suit::Spade,   Rank::Four),
           Card::new(Suit::Heart,   Rank::Five),
           Card::new(Suit::Diamond, Rank::Six),
       ]
   }

   fn bench_evaluate_preflop(c: &mut Criterion) {
       let e = make_evaluator();
       let hole: Vec<u8> = sample_hole().iter().map(|c| c.to_u8()).collect();
       let board: Vec<u8> = vec![];
       c.bench_function("slow_eval/preflop", |b| {
           b.iter(|| black_box(e.evaluate_hand(black_box(&hole), black_box(&board))))
       });
   }

   fn bench_evaluate_river(c: &mut Criterion) {
       let e = make_evaluator();
       let hole: Vec<u8> = sample_hole().iter().map(|c| c.to_u8()).collect();
       let board: Vec<u8> = sample_board().iter().map(|c| c.to_u8()).collect();
       c.bench_function("slow_eval/river", |b| {
           b.iter(|| black_box(e.evaluate_hand(black_box(&hole), black_box(&board))))
       });
   }

   criterion_group!(benches, bench_evaluate_preflop, bench_evaluate_river);
   criterion_main!(benches);
   ```

5. Write `benches/pkr_eval_bench/benches/parity.rs` — this is a **test**, not a bench:
   ```rust
   //! Property test: TableEvaluator and NlheEvaluator must agree on
   //! every (hole, board) input. Runs under the default #[test] harness
   //! so the criterion suite is skipped but cargo test runs it.
   //!
   //! CI: invoked via `cargo test -p pkr-eval-bench --bench parity`.

   use pkr_core::card::{Card, Rank, Suit};
   use pkr_eval::{Evaluator, TableEvaluator};
   use pkr_eval::slow::NlheEvaluator;

   fn sample_cases() -> Vec<(Vec<u8>, Vec<u8>)> {
       let ranks = [
           Rank::Two, Rank::Three, Rank::Four, Rank::Five, Rank::Six,
           Rank::Seven, Rank::Eight, Rank::Nine, Rank::Ten,
           Rank::Jack, Rank::Queen, Rank::King, Rank::Ace,
       ];
       let suits = [Suit::Spade, Suit::Heart, Suit::Diamond, Suit::Club];

       let mut out = Vec::new();
       // 50 fixed (deterministic) hands — enough to catch regressions
       // without being slow.
       for i in 0..50 {
           let h1 = Card::new(suits[i % 4], ranks[(i * 5) % 13]);
           let h2 = Card::new(suits[(i + 1) % 4], ranks[(i * 7 + 3) % 13]);
           let b1 = Card::new(suits[(i + 2) % 4], ranks[(i + 4) % 13]);
           let b2 = Card::new(suits[(i + 3) % 4], ranks[(i + 8) % 13]);
           let b3 = Card::new(suits[i % 4], ranks[(i + 11) % 13]);
           let hole = vec![h1.to_u8(), h2.to_u8()];
           let mut board = vec![b1.to_u8(), b2.to_u8(), b3.to_u8()];
           if i % 2 == 0 { board.push(b1.to_u8()); }
           if i % 3 == 0 { board.push(b2.to_u8()); }
           out.push((hole, board));
       }
       out
   }

   #[test]
   fn table_matches_slow_on_sample_cases() {
       let path = std::env::var("PKR_HAND_RANKS")
           .expect("PKR_HAND_RANKS must point at hand_ranks.bin");
       let table = TableEvaluator::new(&path).unwrap();
       let slow = NlheEvaluator::new();

       for (hole, board) in sample_cases() {
           let t = table.evaluate_hand(&hole, &board);
           let s = slow.evaluate_hand(&hole, &board);
           assert_eq!(t, s, "mismatch on hole={hole:?} board={board:?}");
       }
   }
   ```

6. **Match-against-real-file check:** open `crates/pkr-eval/src/lib.rs`, `slow.rs`, `lookup.rs`, `lookup_fast.rs`. Confirm:
   - `TableEvaluator::new(path)` exists.
   - `NlheEvaluator::new()` exists (if not, find the real constructor and update the bench).
   - The `Evaluator` trait is implemented for both, with `evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32`.
   - `Card::to_u8()` (or equivalent encoding) exists in `crates/pkr-core/src/card.rs`. If cards are passed differently, update.

7. Commit:
   ```bash
   git add benches/pkr_eval_bench/
   git commit -m "B6: add pkr-eval micro-benches (table, slow) + parity test"
   ```

**Acceptance Criteria**
- [ ] `PKR_HAND_RANKS=…/hand_ranks.bin cargo bench -p pkr-eval-bench --bench table` produces output for `preflop` and `river`.
- [ ] Same for `--bench slow`.
- [ ] `PKR_HAND_RANKS=…/hand_ranks.bin cargo test -p pkr-eval-bench --bench parity` passes 50 cases.
- [ ] The parity test runs under `cargo nextest` (it's a `#[test]`, so it will).

---

## B7 — Micro-benches: `pkr-runtime::SolverHandle::get_advice_fast`

**Objective.** The README claims p99 < 1 ms. There is currently no test that hits this. We benchmark the binary search over the sorted-key array for `num_keys = 100, 1k, 100k, 1M`, miss vs hit, and head/tail of the array.

**Exclusive File Paths**
- `benches/pkr_runtime_bench/Cargo.toml` (new)
- `benches/pkr_runtime_bench/benches/lookup.rs` (new)
- `benches/pkr_runtime_bench/benches/build_blueprint.py` (new — generates a synthetic blueprint of given size, or reuse the one smoke.sh produces)

**Dependencies**
- B4, B6.

**Instructions**

1. `mkdir -p benches/pkr_runtime_bench/benches`

2. Write `benches/pkr_runtime_bench/Cargo.toml`:
   ```toml
   [package]
   name = "pkr-runtime-bench"
   version = "0.0.0"
   edition = "2021"
   publish = false

   [dependencies]
   pkr-runtime = { workspace = true }
   pkr-contracts = { workspace = true }

   [dev-dependencies]
   criterion = { workspace = true }

   [[bench]]
   name = "lookup"
   harness = false
   ```

3. Write `benches/pkr_runtime_bench/benches/lookup.rs`:
   ```rust
   //! Benchmark SolverHandle::get_advice_fast — the runtime hot path.
   //!
   //! Three dimensions:
   //!   1. num_keys: 100, 1_000, 100_000, 1_000_000
   //!   2. hit vs miss (miss = key not in the array)
   //!   3. position: head / middle / tail (binary search path length)
   //!
   //! Requires a smoke-built blueprint. We use whatever PKR_BLUEPRINT
   //! points at; if absent, the bench is skipped with a clear error.

   use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
   use pkr_runtime::mmap::MmapReader;
   use pkr_runtime::SolverHandle;
   use std::env;

   fn open_handle() -> SolverHandle {
       let path = env::var("PKR_BLUEPRINT")
           .expect("PKR_BLUEPRINT must point at blueprint.bin (smoke.sh produces one)");
       let rdr = MmapReader::new(&path).expect("failed to mmap blueprint");
       SolverHandle::new(rdr)
   }

   fn keys_from_blueprint(h: &SolverHandle) -> Vec<u64> {
       let bytes = h.debug_keys();
       let n = bytes.len() / 8;
       let mut out = Vec::with_capacity(n);
       for i in 0..n {
           let k = u64::from_le_bytes(bytes[i*8..i*8+8].try_into().unwrap());
           out.push(k);
       }
       out
   }

   fn bench_lookup(c: &mut Criterion) {
       let h = open_handle();
       let keys = keys_from_blueprint(&h);
       if keys.is_empty() {
           eprintln!("WARNING: empty blueprint, skipping lookup bench");
           return;
       }
       let mid = keys.len() / 2;
       let head = 0usize.min(keys.len().saturating_sub(1));
       let tail = keys.len().saturating_sub(1);
       let miss = keys[tail].wrapping_add(1);

       let cases: &[(&str, u64)] = &[
           ("head_hit",  keys[head]),
           ("mid_hit",   keys[mid]),
           ("tail_hit",  keys[tail]),
           ("miss",      miss),
       ];

       let mut group = c.benchmark_group("lookup/get_advice_fast");
       group.sample_size(50);  // larger sample for stable p99
       for &(label, k) in cases {
           group.bench_with_input(BenchmarkId::from_parameter(label), &k, |b, &k| {
               b.iter(|| black_box(h.get_advice_fast(black_box(k))))
           });
       }
       group.finish();
   }

   criterion_group!(benches, bench_lookup);
   criterion_main!(benches);
   ```

4. **Match-against-real-file check:** open `crates/pkr-runtime/src/lib.rs`, `lookup.rs`, `mmap.rs`. Confirm:
   - `SolverHandle::new(MmapReader) -> SolverHandle` exists.
   - `MmapReader::new(path: impl AsRef<Path>) -> Result<MmapReader, MmapError>` exists. (Update if signature differs.)
   - `SolverHandle::debug_keys() -> &[u8]` exists (added per CHANGELOG).
   - `SolverHandle::get_advice_fast(u64) -> Option<SotaAdvice>` exists.

5. Write `benches/pkr_runtime_bench/benches/build_blueprint.py` — optional helper that generates a synthetic 1M-key blueprint for stress-testing without training:
   ```python
   #!/usr/bin/env python3
   """Generate a synthetic blueprint.bin with N sorted u64 keys
   and uniform-random CDF rows, for runtime lookup benchmarking
   without paying the cost of training.

   Usage:
       python3 build_blueprint.py <out_path> <num_keys> <max_actions_k>
   """
   import struct
   import sys
   import random

   MAGIC = b"PKRSOTA1"
   FORMAT_VERSION_V2 = 2
   HASH_ALGO_FNV1A64_INFOSET = 2

   def main():
       out_path = sys.argv[1]
       n_keys = int(sys.argv[2])
       max_actions_k = int(sys.argv[3]) if len(sys.argv) > 3 else 8

       keys = sorted(random.sample(range(0, 1 << 64), n_keys))
       cdf_size = n_keys * max_actions_k

       with open(out_path, "wb") as f:
           # FileHeader: 32 bytes — magic(8) + version(4) + variant(4) +
           # count(4) + k(4) + hash_algo(1) + pad(7)
           f.write(MAGIC)
           f.write(struct.pack("<I", FORMAT_VERSION_V2))
           f.write(struct.pack("<I", 0))  # variant
           f.write(struct.pack("<I", n_keys))
           f.write(struct.pack("<I", max_actions_k))
           f.write(struct.pack("<B", HASH_ALGO_FNV1A64_INFOSET))
           f.write(b"\x00" * 7)  # pad
           # Section: key_count (u32) + cdf_size (u32)
           f.write(struct.pack("<I", n_keys))
           f.write(struct.pack("<I", cdf_size))
           # keys: u64 × n_keys
           for k in keys:
               f.write(struct.pack("<Q", k))
           # cdf: u8 × cdf_size, monotonic per row
           for _ in range(n_keys):
               row = sorted(random.sample(range(0, 256), max_actions_k))
               # normalize to a CDF ending at 255
               cdf = []
               acc = 0
               s = sum(row) or 1
               for v in row:
                   acc += int(v * 255 / s)
                   cdf.append(min(acc, 255))
               cdf[-1] = 255
               f.write(bytes(cdf))

       print(f"wrote {out_path}: {n_keys} keys, cdf_size={cdf_size}")

   if __name__ == "__main__":
       main()
   ```
   **IMPORTANT:** read `crates/pkr-export/src/header.rs` and `writer.rs` and confirm the exact byte layout before trusting this script. If `FileHeader` differs from the comment in this script, update the script and note the mismatch in `worklog.md`.

6. Commit.

**Acceptance Criteria**
- [ ] `PKR_BLUEPRINT=…/blueprint.bin cargo bench -p pkr-runtime-bench` produces output for `head_hit`, `mid_hit`, `tail_hit`, `miss`.
- [ ] p99 of `mid_hit` over 1000 samples is < 1 ms (the README's claim).
- [ ] `build_blueprint.py` produces a file that `MmapReader::new` accepts without an error.

---

## B8 — Micro-benches: `pkr-cfr::CompactRegretTable` ops

**Objective.** The hot per-batch operations on `CompactRegretTable` (insert, snapshot, merge) directly determine training throughput. We isolate: (a) `get_or_create_idx`, (b) `snapshot`, (c) `analyze_strategies`, (d) `sample_infosets`. This catches regressions in the table's lock-free / atomic path.

**Exclusive File Paths**
- `benches/pkr_cfr_bench/Cargo.toml` (new)
- `benches/pkr_cfr_bench/benches/table.rs` (new)
- `benches/pkr_cfr_bench/benches/dcfr.rs` (new — discount formula cost)
- `benches/pkr_cfr_bench/benches/metrics.rs` (new — `record_batch` + `snapshot` + `delta`)

**Dependencies**
- B4, B5.

**Instructions**

1. `mkdir -p benches/pkr_cfr_bench/benches`

2. Write `benches/pkr_cfr_bench/Cargo.toml`:
   ```toml
   [package]
   name = "pkr-cfr-bench"
   version = "0.0.0"
   edition = "2021"
   publish = false

   [dependencies]
   pkr-cfr = { workspace = true }
   pkr-contracts = { workspace = true }

   [dev-dependencies]
   criterion = { workspace = true }

   [[bench]]
   name = "table"
   harness = false

   [[bench]]
   name = "dcfr"
   harness = false

   [[bench]]
   name = "metrics"
   harness = false
   ```

3. Write `benches/pkr_cfr_bench/benches/table.rs`:
   ```rust
   //! Benchmark CompactRegretTable: insert, snapshot, analyze, sample.
   //! Capacity = 100_000 (small enough to fit in L2, isolating logic cost
   //! from memory-bandwidth cost).

   use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
   use pkr_cfr::table::CompactRegretTable;

   fn fill_table(t: &mut CompactRegretTable, n: u64) {
       for i in 0..n {
           let k = i.wrapping_mul(0x9E3779B97F4A7C15);
           let _ = t.get_or_create_idx(k);
       }
   }

   fn bench_get_or_create(c: &mut Criterion) {
       let mut group = c.benchmark_group("table/get_or_create_idx");
       for n in [100_u64, 1_000, 100_000].iter() {
           group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, &n| {
               b.iter_batched(
                   || CompactRegretTable::with_capacity(200_000),
                   |mut t| {
                       for i in 0..n {
                           let k = i.wrapping_mul(0x9E3779B97F4A7C15);
                           let _ = black_box(t.get_or_create_idx(k));
                       }
                   },
                   criterion::BatchSize::SmallInput,
               )
           });
       }
       group.finish();
   }

   fn bench_snapshot(c: &mut Criterion) {
       let mut t = CompactRegretTable::with_capacity(200_000);
       fill_table(&mut t, 100_000);
       c.bench_function("table/snapshot/100k", |b| {
           b.iter(|| black_box(t.snapshot()))
       });
   }

   fn bench_analyze(c: &mut Criterion) {
       let mut t = CompactRegretTable::with_capacity(200_000);
       fill_table(&mut t, 100_000);
       c.bench_function("table/analyze_strategies/100k", |b| {
           b.iter(|| black_box(t.analyze_strategies()))
       });
   }

   criterion_group!(benches, bench_get_or_create, bench_snapshot, bench_analyze);
   criterion_main!(benches);
   ```

4. Write `benches/pkr_cfr_bench/benches/dcfr.rs`:
   ```rust
   //! Benchmark the DCFR discount factor computation. The math is
   //! per-infoset, so a regression here scales with the number of
   //! infosets touched per iteration (~280 nodes/iter × ~27K it/s).

   use criterion::{black_box, criterion_group, criterion_main, Criterion};
   use pkr_cfr::dcfr::{DiscountMode, MomentumMode};

   fn bench_discount_canonical(c: &mut Criterion) {
       c.bench_function("dcfr/canonical/t=1e6", |b| {
           b.iter(|| {
               let t = 1_000_000u64;
               let mode = DiscountMode::CanonicalDcfr;
               let w = mode.weight(t, 1000);
               black_box(w)
           })
       });
   }

   criterion_group!(benches, bench_discount_canonical);
   criterion_main!(benches);
   ```

5. Write `benches/pkr_cfr_bench/benches/metrics.rs`:
   ```rust
   //! Benchmark GlobalMetrics::record_batch + snapshot + delta.

   use criterion::{black_box, criterion_group, criterion_main, Criterion};
   use pkr_cfr::metrics::{GlobalMetrics, LocalMetrics};

   fn bench_record_batch(c: &mut Criterion) {
       let g = GlobalMetrics::new();
       let lm = LocalMetrics::default();
       c.bench_function("metrics/record_batch", |b| {
           b.iter(|| {
               g.record_batch(
                   black_box(&lm),
                   black_box(256),
                   black_box(1_000_000),
                   black_box(900_000),
                   black_box(50_000),
                   black_box(50_000),
                   black_box(1_000),
                   black_box(500),
                   black_box(2_000),
               );
           })
       });
   }

   fn bench_snapshot_delta(c: &mut Criterion) {
       let g = GlobalMetrics::new();
       let lm = LocalMetrics::default();
       g.record_batch(&lm, 256, 1_000_000, 900_000, 50_000, 50_000, 1_000, 500, 2_000);
       let prev = g.snapshot();
       c.bench_function("metrics/snapshot+delta", |b| {
           b.iter(|| {
               let cur = g.snapshot();
               black_box(cur.delta(&prev))
           })
       });
   }

   criterion_group!(benches, bench_record_batch, bench_snapshot_delta);
   criterion_main!(benches);
   ```

6. **Match-against-real-file check:** open `crates/pkr-cfr/src/table.rs`, `dcfr.rs`, `metrics.rs`. Confirm:
   - `CompactRegretTable::with_capacity(usize)` exists (CHANGELOG mentions it).
   - `get_or_create_idx(u64) -> usize` (or similar) exists.
   - `snapshot()`, `analyze_strategies()`, `sample_infosets(n)` exist with the right return types.
   - `DiscountMode::CanonicalDcfr` and `MomentumMode` exist. Confirm the `weight(t, tau)` function signature.
   - `GlobalMetrics::new()` is callable (it is — even though `OnceLock` is used, `new()` is pub per the source).

7. Commit.

**Acceptance Criteria**
- [ ] All three benches (`table`, `dcfr`, `metrics`) run without error.
- [ ] The `record_batch` bench shows sub-microsecond per-call cost (it should — only atomic adds).
- [ ] `dcfr/canonical/t=1e6` runs in nanoseconds (it's a single `pow` + division).

---

## B9 — Micro-benches: `pkr-abstraction::get_infoset_hash`

**Objective.** `get_infoset_hash` is called once per CFR node visited — it's on the same hot path as `fnv1a` and `evaluate_hand`. We bench it per street (preflop, flop, turn, river) because the per-street lookup logic differs.

**Exclusive File Paths**
- `benches/pkr_abstraction_bench/Cargo.toml` (new)
- `benches/pkr_abstraction_bench/benches/abstraction.rs` (new)

**Dependencies**
- B4, B6.

**Instructions**

1. `mkdir -p benches/pkr_abstraction_bench/benches`

2. Write `benches/pkr_abstraction_bench/Cargo.toml`:
   ```toml
   [package]
   name = "pkr-abstraction-bench"
   version = "0.0.0"
   edition = "2021"
   publish = false

   [dependencies]
   pkr-abstraction = { workspace = true }
   pkr-core = { workspace = true }
   pkr-eval = { workspace = true }
   pkr-contracts = { workspace = true }

   [dev-dependencies]
   criterion = { workspace = true }

   [[bench]]
   name = "abstraction"
   harness = false
   ```

3. Write `benches/pkr_abstraction_bench/benches/abstraction.rs`:
   ```rust
   //! Benchmark KMeansAbstraction::get_infoset_hash per street.

   use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
   use pkr_abstraction::{load_centroids, KMeansAbstraction};
   use pkr_eval::TableEvaluator;
   use std::env;

   fn make_abstraction() -> KMeansAbstraction {
       let p = env::var("PKR_HAND_RANKS").expect("PKR_HAND_RANKS");
       let ev = TableEvaluator::new(&p).unwrap();
       let store = load_centroids(&env::var("PKR_CENTROIDS").expect("PKR_CENTROIDS")).unwrap();
       let mut a = KMeansAbstraction::from_store(store, ev);
       // load each street table if env vars are present
       for (street, var) in [(0u8, "PKR_PREFLOP_TABLE"), (1, "PKR_FLOP_TABLE"),
                             (2, "PKR_TURN_TABLE"), (3, "PKR_RIVER_TABLE")] {
           if let Ok(p) = env::var(var) {
               let _ = a.init_table(street, &p);
           }
       }
       a
   }

   fn bench_per_street(c: &mut Criterion) {
       let a = make_abstraction();
       let hole = [0u8, 1u8];           // aces
       let board_empty: Vec<u8> = vec![];
       let board_flop = vec![2u8, 3, 4];
       let board_turn = vec![2u8, 3, 4, 5];
       let board_river = vec![2u8, 3, 4, 5, 6];
       let history = b"cp";   // check-preflop

       let cases: &[(u8, &[u8], &str)] = &[
           (0, &board_empty, "preflop"),
           (1, board_flop.as_slice(), "flop"),
           (2, board_turn.as_slice(), "turn"),
           (3, board_river.as_slice(), "river"),
       ];

       let mut group = c.benchmark_group("abstraction/get_infoset_hash");
       for &(street, board, label) in cases {
           group.bench_with_input(
               BenchmarkId::from_parameter(label),
               &(street, board),
               |b, &(street, board)| {
                   b.iter(|| {
                       black_box(a.get_infoset_hash(
                           black_box(&hole),
                           black_box(board),
                           black_box(history),
                           black_box(street),
                       ))
                   })
               },
           );
       }
       group.finish();
   }

   criterion_group!(benches, bench_per_street);
   criterion_main!(benches);
   ```

4. **Match-against-real-file check:** confirm `KMeansAbstraction::from_store`, `init_table`, `load_centroids` exist with those signatures in `crates/pkr-abstraction/src/lib.rs`. Confirm the `AbstractionBuilder::get_infoset_hash(&self, hole, board, history, street)` trait signature in `crates/pkr-contracts/src/lib.rs`.

5. Commit.

**Acceptance Criteria**
- [ ] All 4 street benches run with env vars set.
- [ ] `preflop` is the cheapest, `river` is the most expensive (it does the most table lookups).
- [ ] p99 of `river` is the dominant single cost per CFR node (matches `docs/status.md` extrapolation).

---

## B10 — `bench.yml` nightly — wraps criterion + thread-scaling

**Objective.** Tie B4-B9 into a nightly CI workflow that produces a stable JSON output, uploads it as an artifact, and (if B11 is done) pushes it to Bencher for trending.

**Exclusive File Paths**
- `.github/workflows/bench.yml` (new)
- `ci/scripts/run-bench.sh` (new)

**Dependencies**
- B4-B9.

**Instructions**

1. Write `ci/scripts/run-bench.sh`:
   ```bash
   #!/usr/bin/env bash
   # Runs every criterion bench and emits NDJSON to $BENCH_OUT (default
   # bench-results.ndjson). Also runs the thread-scaling bench and
   # parses the BENCH lines into a JSON summary.
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   BENCH_OUT="${BENCH_OUT:-bench-results.ndjson}"
   SMOKE_DIR="${SMOKE_DIR:-./outputs/v0-smoke}"

   # 1) Pre-reqs — smoke artifacts must exist (or be restored from cache).
   if [ ! -s "$SMOKE_DIR/hand_ranks.bin" ]; then
       echo "ERROR: $SMOKE_DIR/hand_ranks.bin missing. Run smoke first."
       exit 1
   fi

   export PKR_HAND_RANKS="$SMOKE_DIR/hand_ranks.bin"
   export PKR_CENTROIDS="$SMOKE_DIR/centroids.bin"
   export PKR_BLUEPRINT="$SMOKE_DIR/blueprint.bin"
   export PKR_PREFLOP_TABLE="$SMOKE_DIR/preflop_abstraction.bin"
   export PKR_FLOP_TABLE="$SMOKE_DIR/flop_abstraction.bin"
   export PKR_TURN_TABLE="$SMOKE_DIR/turn_abstraction.bin"
   export PKR_RIVER_TABLE="$SMOKE_DIR/river_buckets.bin"

   : > "$BENCH_OUT"

   # 2) Run each criterion bench. --save-baseline records to
   #    target/criterion for cross-run comparison.
   for BENCH_PKG in pkr-contracts-bench pkr-core-bench pkr-eval-bench \
                    pkr-runtime-bench pkr-cfr-bench pkr-abstraction-bench; do
       echo "==> criterion: $BENCH_PKG"
       cargo bench -p "$BENCH_PKG" --bench "$BENCH_PKG_NAME" \
           -- --save-baseline ci-nightly --output-format=bencher \
           >> "$BENCH_OUT" 2>&1 || true
   done

   # The above loop uses a per-bench name. Easier: run ALL benches in the pkg.
   # Replace with:
   for BENCH_PKG in pkr-contracts-bench pkr-core-bench pkr-eval-bench \
                    pkr-runtime-bench pkr-cfr-bench pkr-abstraction-bench; do
       echo "==> criterion (all benches in pkg): $BENCH_PKG"
       cargo bench -p "$BENCH_PKG" -- \
           --save-baseline ci-nightly --output-format=bencher \
           >> "$BENCH_OUT" 2>&1 || true
   done

   # 3) Thread-scaling bench via bench.sh.
   echo "==> thread scaling"
   THREADS_LIST="1 2 4 8" SECONDS_PER_RUN=10 \
       ./bench.sh > /tmp/bench-scaling.log 2>&1 || true
   python3 ci/scripts/parse-bench-scaling.py /tmp/bench-scaling.log \
       >> "$BENCH_OUT"

   echo "==> bench results in $BENCH_OUT"
   ```

2. Write `ci/scripts/parse-bench-scaling.py`:
   ```python
   #!/usr/bin/env python3
   """Parse bench.sh stdout and emit one Bencher-shaped JSON line per
   threads-config. bench.sh prints `BENCH threads=1 it_per_s=15635`
   style lines (verify against the real bench.sh before trusting).
   """
   import json, re, sys

   pat = re.compile(r"BENCH threads=(\d+) it_per_s=([\d.]+)")
   with open(sys.argv[1]) as f:
       for line in f:
           m = pat.search(line)
           if not m:
               continue
           threads, it_per_s = int(m.group(1)), float(m.group(2))
           print(json.dumps({
               "benchmark": f"trainer/threads_{threads}",
               "value": it_per_s,
               "unit": "it_per_s",
               "higher_is_better": True,
           }))
   ```

3. **Match-against-real-file check:** open `bench.sh`. Confirm the actual `BENCH` line format. The current `bench.sh` greps `iter .*infosets` lines from `pkr-trainer` stderr — there is no literal `BENCH threads=...` string. **Update `parse-bench-scaling.py`** to match the real format, OR modify `bench.sh` to emit `BENCH threads=$T it_per_s=$RATE` lines so the parser works. Pick one and note the decision in `worklog.md`.

4. Write `.github/workflows/bench.yml`:
   ```yaml
   name: bench

   on:
     schedule:
       - cron: '17 3 * * *'   # nightly 03:17 UTC
     workflow_dispatch:

   jobs:
     bench:
       runs-on: ubuntu-22.04
       timeout-minutes: 45
       steps:
         - uses: actions/checkout@v4

         - uses: dtolnay/rust-toolchain@stable

         - name: Cache smoke artifacts
           uses: actions/cache@v4
           with:
             path: outputs/v0-smoke
             key: smoke-abstractions-${{ hashFiles('ci/cache-key.sh', 'smoke.sh') }}

         - name: Compute smoke cache key
           id: key
           run: echo "k=$(./ci/cache-key.sh)" >> $GITHUB_OUTPUT

         - name: Cache smoke (refined)
           uses: actions/cache@v4
           with:
             path: outputs/v0-smoke
             key: smoke-abstractions-${{ runner.os }}-${{ steps.key.outputs.k }}

         - name: Cache cargo
           uses: Swatinem/rust-cache@v2
           with:
             shared-key: bench-release
             cache-targets: true

         - name: Run smoke to ensure artifacts exist
           if: steps.cache-smoke.outputs.cache-hit != 'true'
           run: ./smoke.sh

         - name: Run benches
           run: ./ci/scripts/run-bench.sh
           env:
             BENCH_OUT: bench-results.ndjson

         - name: Upload bench results
           uses: actions/upload-artifact@v4
           with:
             name: bench-results
             path: bench-results.ndjson
             retention-days: 90

         - name: (B11) Push to Bencher
           if: ${{ secrets.BENCHER_API_TOKEN != '' }}
           run: |
             curl -sL https://bencher.dev/download | tar -xz
             ./bencher --token "$BENCH_API_TOKEN" \
                 --project pkr-sota \
                 run \
                 --adapter cargo_bench_json \
                 --file bench-results.ndjson \
                 --branch main \
                 --testbed ci-ubuntu-22.04
   ```

5. Commit.

**Acceptance Criteria**
- [ ] `bench.yml` runs nightly at 03:17 UTC.
- [ ] `ci/scripts/run-bench.sh` produces a non-empty `bench-results.ndjson`.
- [ ] Wall time < 45 min on `ubuntu-22.04`.
- [ ] On failure, the bench-results artifact is still uploaded (so you can see which bench broke).
- [ ] The `--save-baseline ci-nightly` flag persists results to `target/criterion/` so the next night's run can compute deltas.

---

## B11 — Wire Bencher (or branch-history fallback) for trending

**Objective.** Nightly benches produce JSON but nobody reads JSON. We push to Bencher (recommended) OR commit to a `perf-history` branch. Either way, the PR-comment bot in B21 reads from here.

**Exclusive File Paths**
- `ci/bencher.yml` (new — Bencher project config)
- `ci/scripts/diff-perf.sh` (new — branch-history fallback)

**Dependencies**
- B10.

**Instructions**

### Path A: Bencher (recommended)

1. Create a free account at `bencher.dev` (or self-host per their docs).
2. Create a project `pkr-sota`. Note the project slug.
3. Create a testbed `ci-ubuntu-22.04` matching the runner.
4. Generate an API token. Add it as a GitHub repo secret named `BENCHER_API_TOKEN`.
5. Write `ci/bencher.yml` (the project-level config — also lives in the repo for portability):
   ```yaml
   project: pkr-sota
   testbed: ci-ubuntu-22.04
   branch: main
   # Adapters map bench output formats to Bencher's metric model.
   adapter: cargo_bench_json
   # Thresholds: PR-comment warns at 5% regression, alerts at 20%.
   thresholds:
     - name: default
       metric: latency
       upper_limit: 1.20    # 20% slower
       lower_limit: 0.83    # 17% faster (catches test bugs)
   ```
6. `bench.yml` (from B10) already pushes via `./bencher … run`. Done.

### Path B: Branch-history fallback

1. Write `ci/scripts/diff-perf.sh`:
   ```bash
   #!/usr/bin/env bash
   # Reads the last two nightly bench-results.ndjson files from the
   # perf-history branch and prints a Markdown table.
   # Usage: ./ci/scripts/diff-perf.sh [old.json] [new.json]
   set -euo pipefail
   OLD="${1:-}"
   NEW="${2:-}"
   if [ -z "$OLD" ] || [ -z "$NEW" ]; then
       echo "Usage: $0 <old.ndjson> <new.ndjson>"
       exit 1
   fi

   python3 - "$OLD" "$NEW" <<'PY'
   import json, sys
   old = {json.loads(l)["benchmark"]: json.loads(l) for l in open(sys.argv[1]) if l.strip()}
   new = {json.loads(l)["benchmark"]: json.loads(l) for l in open(sys.argv[2]) if l.strip()}
   keys = sorted(set(old) | set(new))
   print("| benchmark | old | new | Δ% | note |")
   print("|---|---|---|---|---|")
   for k in keys:
       o = old.get(k, {}).get("value")
       n = new.get(k, {}).get("value")
       if o is None or n is None:
           print(f"| `{k}` | {o} | {n} | — | missing |")
           continue
       pct = (n - o) / o * 100
       arrow = "🔴" if pct > 5 else ("🟢" if pct < -5 else "→")
       print(f"| `{k}` | {o:.1f} | {n:.1f} | {pct:+.1f}% {arrow} | |")
   PY
   ```

2. Add a new workflow `.github/workflows/commit-bench.yml`:
   ```yaml
   name: commit-bench

   on:
     workflow_run:
       workflows: [bench]
       types: [completed]

   jobs:
     commit:
       runs-on: ubuntu-22.04
       if: ${{ github.event.workflow_run.conclusion == 'success' }}
       steps:
         - uses: actions/checkout@v4
           with:
             ref: perf-history
             fetch-depth: 0

         - name: Download bench artifact
           uses: actions/download-artifact@v4
           with:
             name: bench-results
             path: nightly
             run_id: ${{ github.event.workflow_run.id }}
             github-token: ${{ secrets.GITHUB_TOKEN }}

         - name: Move to dated path
           run: |
             DATE=$(date -u +%Y-%m-%d)
             mkdir -p "nightly/$DATE"
             mv nightly/bench-results.ndjson "nightly/$DATE/bench-results.ndjson"

         - name: Commit
           run: |
             git config user.email "ci@pkr-sota"
             git config user.name "ci-bot"
             git add nightly/
             git commit -m "nightly bench $(date -u +%Y-%m-%d)" || true
             git push
   ```

3. Create the `perf-history` branch manually once:
   ```bash
   git checkout --orphan perf-history
   git rm -rf .
   echo "# perf history" > README.md
   git add README.md
   git commit -m "init perf-history"
   git push origin perf-history
   git checkout main
   ```

4. Commit the `diff-perf.sh` script.

**Acceptance Criteria (both paths)**
- [ ] Nightly bench produces a Bencher report OR a committed JSON on `perf-history`.
- [ ] Bencher's PR-comment feature OR `diff-perf.sh` prints a Markdown table comparing two runs.
- [ ] A 5% regression is flagged with a red arrow / 🔴.
- [ ] An alert email/notification fires at 20% regression.

---

# TIER 2 — Integration benchmarks + quality metrics

## B12 — `proftest-ci.yml` — production-scale profile in CI

**Objective.** `proftest.sh` runs 100K iterations of real training. We run it nightly on CI, parse the resulting `metrics.csv` and `stats.json`, and emit Bencher custom metrics. This is the **throughput trend** source — the number that tells you if training is getting faster or slower.

**Exclusive File Paths**
- `.github/workflows/proftest-ci.yml` (new)
- `ci/scripts/run-proftest-ci.sh` (new)

**Dependencies**
- B1, B10.

**Instructions**

1. Write `ci/scripts/run-proftest-ci.sh`:
   ```bash
   #!/usr/bin/env bash
   # CI-friendly proftest. Cuts iteration count to 50K (still gives a
   # stable it/s signal) and 4 threads (CI runner constraint).
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   PROF_DIR="${PROF_DIR:-./outputs/v0-proftest-ci}"
   ITERATIONS="${ITERATIONS:-50000}"
   THREADS="${THREADS:-4}"
   CAPACITY="${CAPACITY:-5000000}"

   export PROF_DIR ITERATIONS THREADS CAPACITY

   # Run the existing proftest.sh — it already does precompute-cache +
   # train + JSON validate + artifact listing.
   ./proftest.sh

   # Emit a tiny summary JSON for Bencher custom metrics (B13 parses
   # the full metrics.csv too, but this is the one-number headline).
   python3 - "$PROF_DIR" <<'PY'
   import json, os, sys, csv
   prof = sys.argv[1]
   # Last row of metrics.csv has the cumulative it/s.
   with open(os.path.join(prof, "metrics.csv")) as f:
       rows = list(csv.DictReader(f))
       last = rows[-1] if rows else {}
       it_per_s = float(last.get("it_per_s", 0))
       cache_hit = float(last.get("cache_hit_rate", 0))
       cap_pct = float(last.get("cap_pct", 0))
   # stats.json has end-of-run snapshot
   with open(os.path.join(prof, "stats.json")) as f:
       stats = json.load(f)
   snap = stats.get("snapshot", {})
   cum = stats.get("cumulative_metrics", {})
   print(json.dumps({
       "iterations": int(last.get("iter", 0)),
       "it_per_s": it_per_s,
       "cache_hit_rate": cache_hit,
       "capacity_pct": cap_pct,
       "infosets": snap.get("infosets", 0),
       "max_abs_regret": snap.get("max_abs_regret", 0),
       "nonfinite_count": snap.get("nonfinite_count", 0),
       "nodes_per_iteration": cum.get("nodes_per_iteration", 0),
       "avg_depth": cum.get("avg_depth", 0),
       "max_depth": cum.get("max_depth", 0),
   }, indent=2))
   PY
   ```

2. Write `.github/workflows/proftest-ci.yml`:
   ```yaml
   name: proftest

   on:
     schedule:
       - cron: '47 3 * * *'   # nightly 03:47 UTC (after bench starts)
     workflow_dispatch:

   jobs:
     proftest:
       runs-on: ubuntu-22.04
       timeout-minutes: 30
       steps:
         - uses: actions/checkout@v4

         - uses: dtolnay/rust-toolchain@stable

         - name: Cache cargo
           uses: Swatinem/rust-cache@v2
           with:
             shared-key: proftest-release
             cache-targets: true

         - name: Cache proftest artifacts
           uses: actions/cache@v4
           with:
             path: outputs/v0-proftest-ci
             key: proftest-${{ hashFiles('ci/cache-key.sh', 'proftest.sh') }}

         - name: Compute cache key
           id: key
           run: echo "k=$(./ci/cache-key.sh)" >> $GITHUB_OUTPUT

         - name: Cache proftest (refined)
           uses: actions/cache@v4
           with:
             path: outputs/v0-proftest-ci
             key: proftest-${{ runner.os }}-${{ steps.key.outputs.k }}

         - name: Run proftest
           run: ./ci/scripts/run-proftest-ci.sh

         - name: Upload artifacts
           uses: actions/upload-artifact@v4
           with:
             name: proftest
             path: |
               outputs/v0-proftest-ci/metrics.csv
               outputs/v0-proftest-ci/stats.json
               outputs/v0-proftest-ci/blueprint.bin
             retention-days: 30
   ```

3. Commit.

**Acceptance Criteria**
- [ ] `proftest-ci.yml` runs nightly at 03:47 UTC.
- [ ] `metrics.csv`, `stats.json`, `blueprint.bin` are uploaded as artifacts.
- [ ] The summary JSON prints to stdout (and is captured by the workflow log).
- [ ] Wall time < 30 min on `ubuntu-22.04` 4-vCPU.

---

## B13 — Parse `metrics.csv` + `stats.json` into Bencher custom metrics

**Objective.** `proftest.sh` produces a rich `metrics.csv` (22 columns × ~10-50 rows per run) and a `stats.json` with cumulative totals. We push each interesting column as a separate Bencher metric so the dashboard can trend them.

**Exclusive File Paths**
- `ci/scripts/parse-metrics-csv.py` (new)
- `ci/scripts/parse-stats-json.py` (new)
- `ci/scripts/push-custom-metrics.sh` (new)

**Dependencies**
- B11, B12.

**Instructions**

1. Write `ci/scripts/parse-metrics-csv.py`:
   ```python
   #!/usr/bin/env python3
   """Parse metrics.csv and emit one Bencher-shaped JSON line per
   (column, last-row) pair.

   Output goes to stdout; consumed by `bencher run` or by the
   branch-history fallback.
   """
   import csv, json, sys

   if len(sys.argv) < 2:
       print("usage: parse-metrics-csv.py <metrics.csv>", file=sys.stderr)
       sys.exit(1)

   path = sys.argv[1]
   with open(path) as f:
       rows = list(csv.DictReader(f))
   if not rows:
       sys.exit(0)

   last = rows[-1]

   # Columns to trend (see binaries/pkr-trainer/src/main.rs CSV header).
   COLUMNS = [
       # (column, unit, higher_is_better)
       ("it_per_s",        "it_per_s", True),
       ("infosets",        "count",    True),
       ("cap_pct",         "pct",      False),
       ("max_abs_regret",  "abs",      False),
       ("mean_abs_regret", "abs",      False),
       ("nonfinite",       "count",    False),
       ("strat_mass",      "mass",     None),
       ("nodes_per_iter",  "count",    None),
       ("avg_depth",       "depth",    None),
       ("max_depth",       "depth",    None),
       ("cache_hit_rate",  "ratio",    True),
       ("regret_in",       "count",    None),
       ("regret_out",      "count",    None),
       ("regret_dedup",    "ratio",    True),
       ("strategy_applied","count",    None),
       ("traverse_ms",     "ms",       False),
       ("merge_ms",        "ms",       False),
       ("flush_ms",        "ms",       False),
       ("wall_ms",         "ms",       False),
   ]

   for col, unit, hib in COLUMNS:
       if col not in last or not last[col]:
           continue
       try:
           v = float(last[col])
       except ValueError:
           continue
       print(json.dumps({
           "benchmark": f"proftest/{col}",
           "value": v,
           "unit": unit,
           "higher_is_better": hib,
       }))
   ```

2. Write `ci/scripts/parse-stats-json.py`:
   ```python
   #!/usr/bin/env python3
   """Parse stats.json and emit Bencher-shaped JSON lines for
   cumulative metrics that aren't in metrics.csv's last row.
   """
   import json, sys

   if len(sys.argv) < 2:
       print("usage: parse-stats-json.py <stats.json>", file=sys.stderr)
       sys.exit(1)

   with open(sys.argv[1]) as f:
       s = json.load(f)

   def emit(name, value, unit, hib=None):
       if value is None:
           return
       try:
           v = float(value)
       except (TypeError, ValueError):
           return
       print(json.dumps({
           "benchmark": f"stats/{name}",
           "value": v,
           "unit": unit,
           "higher_is_better": hib,
       }))

   snap = s.get("snapshot", {})
   cum  = s.get("cumulative_metrics", {})
   sa   = s.get("strategy_analysis", {})

   emit("wall_seconds",          s.get("wall_seconds"),          "s",     False)
   emit("infosets",              snap.get("infosets"),           "count", True)
   emit("capacity_pct",          snap.get("capacity_pct"),       "pct",   False)
   emit("max_abs_regret",        snap.get("max_abs_regret"),     "abs",   False)
   emit("mean_abs_regret",       snap.get("mean_abs_regret"),    "abs",   False)
   emit("nonfinite_count",       snap.get("nonfinite_count"),    "count", False)
   emit("strategy_sum_mass",    snap.get("strategy_sum_mass"),  "mass",  None)
   emit("nodes_per_iteration",  cum.get("nodes_per_iteration"), "count", None)
   emit("avg_depth",             cum.get("avg_depth"),           "depth", None)
   emit("max_depth",             cum.get("max_depth"),           "depth", None)
   emit("cache_hit_rate",       cum.get("cache_hit_rate"),      "ratio", True)
   emit("infosets_created",     cum.get("infosets_created"),    "count", True)
   emit("regret_dedup_ratio",   cum.get("regret_dedup_ratio"), "ratio", True)
   emit("total_traverse_s",     cum.get("total_traverse_s"),    "s",     False)
   emit("total_merge_s",        cum.get("total_merge_s"),       "s",     False)
   emit("total_flush_s",        cum.get("total_flush_s"),       "s",     False)
   emit("mean_entropy_bits",    sa.get("mean_entropy_bits"),    "bits",  None)
   emit("strategy_pure",        sa.get("pure"),                 "count", None)
   emit("strategy_mixed",       sa.get("mixed"),                "count", None)
   emit("strategy_empty",       sa.get("empty"),                "count", False)
   ```

3. Write `ci/scripts/push-custom-metrics.sh`:
   ```bash
   #!/usr/bin/env bash
   # Compose parse-metrics-csv.py + parse-stats-json.py output, then
   # push to Bencher (if BENCHER_API_TOKEN set) or save to disk.
   set -euo pipefail
   PROF_DIR="${1:-./outputs/v0-proftest-ci}"
   OUT="${2:-custom-metrics.ndjson}"

   : > "$OUT"
   python3 ci/scripts/parse-metrics-csv.py "$PROF_DIR/metrics.csv" >> "$OUT"
   python3 ci/scripts/parse-stats-json.py  "$PROF_DIR/stats.json"  >> "$OUT"

   if [ -n "${BENCHER_API_TOKEN:-}" ]; then
       ./bencher --token "$BENCHER_API_TOKEN" \
           --project pkr-sota \
           run \
           --adapter json \
           --file "$OUT" \
           --branch main \
           --testbed ci-ubuntu-22.04
   fi
   ```

4. Add a step to `proftest-ci.yml` after the run:
   ```yaml
         - name: Push custom metrics
           run: ./ci/scripts/push-custom-metrics.sh
   ```

5. Commit.

**Acceptance Criteria**
- [ ] `parse-metrics-csv.py` produces ~19 JSON lines from a `metrics.csv` with the standard 22-column header.
- [ ] `parse-stats-json.py` produces ~20 JSON lines from `stats.json`.
- [ ] `push-custom-metrics.sh` succeeds and either uploads to Bencher or saves the file.
- [ ] Bencher shows `proftest/it_per_s` as a trended metric.

---

## B14 — Kuhn exploitability tracked as a CI metric

**Objective.** `crates/pkr-testgames/src/bin/kuhn_experiment.rs` emits an exploitability table at log-spaced checkpoints. We parse stdout and trend each `(config, checkpoint)` pair. This is the **convergence-quality** metric — the one that catches algorithmic regressions that throughput can't see.

**Exclusive File Paths**
- `ci/scripts/parse-kuhn.py` (new)
- `ci/scripts/run-kuhn.sh` (new)

**Dependencies**
- B11.

**Instructions**

1. Write `ci/scripts/run-kuhn.sh`:
   ```bash
   #!/usr/bin/env bash
   # Run kuhn_experiment.rs and capture stdout for parsing.
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   OUT="${OUT:-kuhn-results.txt}"
   cargo run --release -p pkr-testgames --bin kuhn-experiment > "$OUT" 2>&1
   ```

2. Write `ci/scripts/parse-kuhn.py`:
   ```python
   #!/usr/bin/env python3
   """Parse kuhn_experiment.rs stdout into Bencher-shaped JSON lines.

   The expected stdout format (from crates/pkr-testgames/src/bin/kuhn_experiment.rs):
       === Kuhn poker: discount x momentum ===
       Nash value to P0: -1/18 = -0.055556

                iter      vanilla        van-mom          canon      canon-mom
                 100   1.234e-01    1.234e-01    1.234e-01    1.234e-01
                 300   1.234e-01    ...
                 ...

       === Final values at t=3000000 ===
         vanilla       expl=1.234e-03  value=-0.055556  max|reg|=...
         ...

   We emit one metric per (config, checkpoint) — `kuhn/<config>@<iter>`.
   """
   import json, re, sys

   if len(sys.argv) < 2:
       print("usage: parse-kuhn.py <kuhn-results.txt>", file=sys.stderr)
       sys.exit(1)

   # Config labels are read from the header row.
   cfg_pat = re.compile(r"^\s*iter\s+(.+?)\s*$")
   row_pat = re.compile(r"^\s*(\d+)\s+(.+?)\s*$")
   nan_pat = re.compile(r"^\s*NaN\s*$")

   configs: list[str] = []
   with open(sys.argv[1]) as f:
       for line in f:
           m = cfg_pat.match(line)
           if m:
               configs = m.group(1).split()
               continue
           m = row_pat.match(line)
           if not m or not configs:
               continue
           it = int(m.group(1))
           rest = m.group(2).split()
           if len(rest) < len(configs):
               continue
           for i, cfg in enumerate(configs):
               tok = rest[i]
               if nan_pat.match(tok):
                   # NaN is treated as a failure (very high exploitability).
                   v = 1.0e9
               else:
                   try:
                       v = float(tok)
                   except ValueError:
                       continue
               print(json.dumps({
                   "benchmark": f"kuhn/{cfg}@{it}",
                   "value": v,
                   "unit": "exploitability",
                   "higher_is_better": False,
               }))
   ```

3. Add a step to `bench.yml` (B10) after the bench results upload:
   ```yaml
         - name: Run Kuhn experiment
           run: ./ci/scripts/run-kuhn.sh
           env:
             OUT: kuhn-results.txt

         - name: Parse Kuhn output
           run: python3 ci/scripts/parse-kuhn.py kuhn-results.txt > kuhn-metrics.ndjson

         - name: Upload Kuhn metrics
           uses: actions/upload-artifact@v4
           with:
             name: kuhn-metrics
             path: kuhn-metrics.ndjson
             retention-days: 90

         - name: Push Kuhn to Bencher
           if: ${{ secrets.BENCHER_API_TOKEN != '' }}
           run: |
             ./bencher --token "$BENCHER_API_TOKEN" \
                 --project pkr-sota \
                 run \
                 --adapter json \
                 --file kuhn-metrics.ndjson \
                 --branch main \
                 --testbed ci-ubuntu-22.04
   ```

4. Commit.

**Acceptance Criteria**
- [ ] `kuhn-metrics.ndjson` contains ≥ 40 lines (4 configs × 10 checkpoints).
- [ ] `kuhn/canon-mom@3000000` is the lowest (best) value.
- [ ] A regression in the canon/canon-mom configs is visible as a +X% delta in Bencher.
- [ ] The `vanilla` config at t=3M stays above the `canon-mom` value (sanity).

---

## B15 — Sampled-BR exploitability tracked for NLHE

**Objective.** `pkr_exploit::best_response::sampled_exploitability` runs inside the trainer when `--eval-every > 0`. We extract its output (the `EVAL iter=… expl_mbb=… br0=… br1_to_p0=… deals=…` stderr lines) from proftest and trend it. This is the **NLHE quality metric** that complements Kuhn's perfect-knowledge metric.

**Exclusive File Paths**
- `ci/scripts/parse-eval-stderr.py` (new)
- `binaries/pkr-trainer/src/main.rs` (modified — `--eval-every 10000` default in proftest, see B12)

**Dependencies**
- B11, B12, B14.

**Instructions**

1. First, modify `proftest.sh` to enable the eval loop. In the existing `cargo run` block of `proftest.sh`, add `--eval-every 10000 --eval-deals 2000` after `--report-every 5000`:
   ```bash
       --report-every 5000 \
       --eval-every 10000 \
       --eval-deals 2000 \
   ```
   Also redirect stderr to a captured file (currently `proftest.sh` lets `cargo run` write to the terminal; we need to capture it):
   ```bash
   time cargo run --release -p pkr-trainer -- \
       ... \
       --stats-json "$PROF_DIR_ABS/stats.json" \
       2> "$PROF_DIR_ABS/trainer.stderr"
   ```
   This is safe — `eprintln!` already writes to stderr; we just persist it.

2. Write `ci/scripts/parse-eval-stderr.py`:
   ```python
   #!/usr/bin/env python3
   """Parse trainer stderr lines of the form:
       EVAL iter=10000 expl_mbb=1234.56 br0=0.12 br1_to_p0=-0.34 deals=2000
   and emit one Bencher line per (iter, metric).
   """
   import json, re, sys

   pat = re.compile(
       r"EVAL iter=(\d+) expl_mbb=([\d.eE+-]+) br0=([\d.eE+-]+) "
       r"br1_to_p0=([\d.eE+-]+) deals=(\d+)"
   )

   with open(sys.argv[1]) as f:
       for line in f:
           m = pat.search(line)
           if not m:
               continue
           it = int(m.group(1))
           for name, val, hib in [
               ("expl_mbb", float(m.group(2)), False),
               ("br0",      float(m.group(3)), None),
               ("br1_to_p0",float(m.group(4)), None),
           ]:
               print(json.dumps({
                   "benchmark": f"nlhe_eval/{name}@{it}",
                   "value": val,
                   "unit": name,
                   "higher_is_better": hib,
               }))
   ```

3. Add to `proftest-ci.yml` (B12):
   ```yaml
         - name: Parse eval stderr
           run: |
             python3 ci/scripts/parse-eval-stderr.py \
                 outputs/v0-proftest-ci/trainer.stderr \
                 > eval-metrics.ndjson
             cat eval-metrics.ndjson
         - name: Upload eval metrics
           uses: actions/upload-artifact@v4
           with:
             name: eval-metrics
             path: eval-metrics.ndjson
             retention-days: 30
         - name: Push eval to Bencher
           if: ${{ secrets.BENCHER_API_TOKEN != '' }}
           run: |
             ./bencher --token "$BENCHER_API_TOKEN" \
                 --project pkr-sota \
                 run \
                 --adapter json \
                 --file eval-metrics.ndjson \
                 --branch main \
                 --testbed ci-ubuntu-22.04
   ```

4. Commit.

**Acceptance Criteria**
- [ ] `eval-metrics.ndjson` contains 5×3 = 15 lines (5 eval checkpoints × 3 metrics) from a 50K-iter run with `--eval-every 10000`.
- [ ] `expl_mbb` decreases over iterations (sanity: a regression that *increases* it is bad).
- [ ] `deals` stays 2000 (or whatever `--eval-deals` is set to).

---

## B16 — Binary-size + compile-time tracking

**Objective.** Two cheap metrics that catch surprising regressions: a dep that bloats the binary, an `#[inline]` that explodes compile time. Both run in seconds and fit in the nightly bench.

**Exclusive File Paths**
- `ci/scripts/measure-binary-size.sh` (new)
- `ci/scripts/measure-compile-time.sh` (new)

**Dependencies**
- B11.

**Instructions**

1. Write `ci/scripts/measure-binary-size.sh`:
   ```bash
   #!/usr/bin/env bash
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   OUT="${OUT:-binary-size.json}"
   cargo build --release -p pkr-trainer -p pkr-abstraction --bins

   emit() {
       local name="$1" path="$2"
       local size
       size=$(wc -c < "$path" | tr -d ' ')
       python3 -c "
       import json,sys
       print(json.dumps({
           'benchmark': 'binary_size/$name',
           'value': $size,
           'unit': 'bytes',
           'higher_is_better': False,
       }))"
   }

   {
       emit pkr-trainer            target/release/pkr-trainer
       emit pkr-abstraction-precompute target/release/pkr-abstraction-precompute
       emit pkr-abstraction-lib      target/release/libpkr_abstraction.rlib 2>/dev/null || true
   } > "$OUT"
   cat "$OUT"
   ```

2. Write `ci/scripts/measure-compile-time.sh`:
   ```bash
   #!/usr/bin/env bash
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   OUT="${OUT:-compile-time.json}"
   rm -rf target/release  # cold compile to get a fair number
   T_START=$(date +%s.%N)
   cargo build --release -p pkr-trainer -p pkr-abstraction --bins
   T_END=$(date +%s.%N)
   python3 -c "
   import json
   elapsed = $T_END - $T_START
   print(json.dumps({
       'benchmark': 'compile_time/release_cold',
       'value': elapsed,
       'unit': 's',
       'higher_is_better': False,
   }))
   " > "$OUT"
   cat "$OUT"
   ```

3. Add to `bench.yml` (B10) after the bench results upload:
   ```yaml
         - name: Measure binary size
           run: ./ci/scripts/measure-binary-size.sh
           env:
             OUT: binary-size.json
         - name: Measure compile time
           run: ./ci/scripts/measure-compile-time.sh
           env:
             OUT: compile-time.json
         - name: Push binary-size + compile-time to Bencher
           if: ${{ secrets.BENCHER_API_TOKEN != '' }}
           run: |
             cat binary-size.json >> bench-results.ndjson
             cat compile-time.json >> bench-results.ndjson
   ```

4. Commit.

**Acceptance Criteria**
- [ ] `binary-size.json` contains ≥ 2 entries (pkr-trainer, pkr-abstraction-precompute).
- [ ] `compile-time.json` contains the cold-compile wall time.
- [ ] Bencher trends both.

---

## B17 — Coverage via `cargo-llvm-cov` in `weekly.yml`

**Objective.** "98 tests pass" tells you nothing about coverage. We add weekly coverage reporting and trend the % covered per crate.

**Exclusive File Paths**
- `.github/workflows/weekly.yml` (new — full workflow)
- `ci/scripts/run-coverage.sh` (new)

**Dependencies**
- B1.

**Instructions**

1. Write `ci/scripts/run-coverage.sh`:
   ```bash
   #!/usr/bin/env bash
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   cargo llvm-cov --workspace --lcov --output-path lcov.info

   # Emit per-crate % as Bencher metrics.
   python3 - <<'PY'
   import json, re, sys
   # Parse `cargo llvm-cov --workspace --summary` for per-crate %.
   # The summary format is:
   #   Crate         Lines   Replaced  ...  Region %
   #   pkr-core      123/130 0         ...  94.6%
   # We re-run with --summary to get this; lcov.info above is for codecov.
   import subprocess
   r = subprocess.run(
       ["cargo", "llvm-cov", "--workspace", "--summary"],
       capture_output=True, text=True, check=True,
   )
   for line in r.stdout.splitlines():
       m = re.match(r"\s*(pkr-\S+)\s+.*?\s+(\d+\.\d+)%\s*$", line)
       if not m: continue
       crate, pct = m.group(1), float(m.group(2))
       print(json.dumps({
           "benchmark": f"coverage/{crate}",
           "value": pct,
           "unit": "pct",
           "higher_is_better": True,
       }))
   PY
   ```

2. Write `.github/workflows/weekly.yml`:
   ```yaml
   name: weekly

   on:
     schedule:
       - cron: '17 2 * * 1'   # Monday 02:17 UTC
     workflow_dispatch:

   jobs:
     weekly:
       runs-on: ubuntu-22.04
       timeout-minutes: 120
       steps:
         - uses: actions/checkout@v4
         - uses: dtolnay/rust-toolchain@stable
         - uses: taiki-e/install-action@v2
           with:
             tool: cargo-llvm-cov
         - uses: Swatinem/rust-cache@v2
           with:
             shared-key: weekly
             cache-targets: true

         - name: Coverage
           run: ./ci/scripts/run-coverage.sh > coverage.ndjson
         - name: Upload lcov
           uses: actions/upload-artifact@v4
           with:
             name: lcov
             path: lcov.info
             retention-days: 30
         - name: Push coverage to Bencher
           if: ${{ secrets.BENCHER_API_TOKEN != '' }}
           run: |
             ./bencher --token "$BENCHER_API_TOKEN" \
                 --project pkr-sota \
                 run --adapter json --file coverage.ndjson \
                 --branch main --testbed ci-ubuntu-22.04
   ```

3. Commit.

**Acceptance Criteria**
- [ ] `weekly.yml` triggers Monday 02:17 UTC.
- [ ] `coverage.ndjson` contains ≥ 11 entries (one per crate).
- [ ] `lcov.info` is uploaded and parseable by `lcov --summary lcov.info`.
- [ ] Bencher trends coverage per crate.

---

# TIER 3 — Deep checks

## B18 — Memory profile with `dhat` in `weekly.yml`

**Objective.** The 50M-capacity training run claims a ~600 MB working set (per `docs/README.md`). `dhat` gives us a real number, plus a per-allocation-site breakdown. We add this to weekly CI.

**Exclusive File Paths**
- `ci/scripts/run-dhat.sh` (new)
- `binaries/pkr-trainer/Cargo.toml` (modified — add a `dhat` feature)

**Dependencies**
- B17.

**Instructions**

1. Add a `dhat` feature to `binaries/pkr-trainer/Cargo.toml`:
   ```toml
   [features]
   dhat-profiling = ["dhat"]

   [dependencies]
   # existing deps...
   dhat = { version = "0.3", optional = true }
   ```

2. In `binaries/pkr-trainer/src/main.rs`, wrap the `run()` entry with optional dhat boilerplate. At the top of the file:
   ```rust
   #[cfg(feature = "dhat-profiling")]
   use dhat::{Dhat, DhatAlloc};

   #[cfg(feature = "dhat-profiling")]
   #[global_allocator]
   static ALLOC: DhatAlloc = DhatAlloc;

   #[cfg(feature = "dhat-profiling")]
   static DHAT: Dhat = Dhat::new_heap();
   ```

   At the end of `run()` (before the final `Ok(())`):
   ```rust
   #[cfg(feature = "dhat-profiling")]
   {
       let _ = std::fs::create_dir("dhat-out");
       dhat::to_file(&DHAT, "dhat-out/dhat-heap.json", None)
           .expect("failed to write dhat output");
   }
   ```

3. Write `ci/scripts/run-dhat.sh`:
   ```bash
   #!/usr/bin/env bash
   # Run a short (5K-iter) proftest with dhat profiling enabled.
   # Parse the resulting JSON for total bytes + top alloc sites.
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   PROF_DIR="${PROF_DIR:-./outputs/v0-dhat}"
   mkdir -p "$PROF_DIR"

   cargo build --release -p pkr-trainer --features dhat-profiling
   # Note: dhat disables LTO by overriding the profile; we accept this
   # because the goal is to measure memory, not throughput.

   PROF_DIR="$PROF_DIR" \
   ITERATIONS=5000 THREADS=2 CAPACITY=1000000 \
       ./proftest.sh

   python3 - "$PROF_DIR/dhat-out/dhat-heap.json" <<'PY'
   import json, sys
   with open(sys.argv[1]) as f:
       d = json.load(f)
   total = d.get("total_bytes", 0)
   peak  = d.get("peak_bytes", 0)
   print(json.dumps({
       "benchmark": "memory/total_bytes",
       "value": total,
       "unit": "bytes",
       "higher_is_better": False,
   }))
   print(json.dumps({
       "benchmark": "memory/peak_bytes",
       "value": peak,
       "unit": "bytes",
       "higher_is_better": False,
   }))
   # Top 5 allocation sites by total bytes
   items = sorted(d.get("items", []), key=lambda x: -x.get("total_bytes", 0))[:5]
   for it in items:
       print(json.dumps({
           "benchmark": f"memory/site_{it['frame'][:60]}",
           "value": it.get("total_bytes", 0),
           "unit": "bytes",
           "higher_is_better": False,
       }))
   PY
   ```

4. Add to `weekly.yml` (B17) after coverage:
   ```yaml
         - name: Memory profile
           run: ./ci/scripts/run-dhat.sh > memory.ndjson
         - name: Upload dhat
           uses: actions/upload-artifact@v4
           with:
             name: dhat-heap
             path: outputs/v0-dhat/dhat-out/dhat-heap.json
             retention-days: 30
         - name: Push memory metrics
           if: ${{ secrets.BENCHER_API_TOKEN != '' }}
           run: |
             ./bencher --token "$BENCHER_API_TOKEN" \
                 --project pkr-sota \
                 run --adapter json --file memory.ndjson \
                 --branch main --testbed ci-ubuntu-22.04
   ```

5. Commit.

**Acceptance Criteria**
- [ ] `--features dhat-profiling` builds cleanly.
- [ ] `dhat-heap.json` is produced for a 5K-iter run.
- [ ] `memory.ndjson` contains `total_bytes`, `peak_bytes`, and ≥ 5 site entries.
- [ ] Bencher trends both total and peak.

---

## B19 — Fuzz run wired into `weekly.yml`

**Objective.** `crates/pkr-fuzz` is defined but "unwired" per `docs/README.md`. We add a `cargo fuzz` target that runs for 10 minutes weekly.

**Exclusive File Paths**
- `crates/pkr-fuzz/Cargo.toml` (modified — enable cargo-fuzz)
- `crates/pkr-fuzz/fuzz/Cargo.toml` (new)
- `crates/pkr-fuzz/fuzz/fuzz_targets/blueprint_loader.rs` (new)
- `crates/pkr-fuzz/fuzz/fuzz_targets/state_transitions.rs` (new)
- `ci/scripts/run-fuzz.sh` (new)

**Dependencies**
- B17.

**Instructions**

1. Read `crates/pkr-fuzz/src/lib.rs` and `Cargo.toml`. Determine the existing API surface — there's likely already a fuzz-like harness. If `cargo-fuzz` is set up, skip step 2.

2. If `crates/pkr-fuzz/fuzz/` doesn't exist, set it up:
   ```bash
   cd crates/pkr-fuzz
   cargo init --lib --fuzz
   ```
   Wait — `cargo-fuzz` requires its own setup. The convention is:
   ```bash
   cargo install cargo-fuzz
   cd crates/pkr-fuzz
   cargo fuzz init
   ```
   This creates `fuzz/Cargo.toml` and `fuzz/fuzz_targets/fuzz_target_1.rs`. Replace that file's contents with real targets.

3. Write `crates/pkr-fuzz/fuzz/Cargo.toml`:
   ```toml
   [package]
   name = "pkr-fuzz-fuzz"
   version = "0.0.0"
   edition = "2021"
   publish = false

   [dependencies]
   pkr-runtime = { workspace = true }
   pkr-export = { workspace = true }
   pkr-core = { workspace = true }
   pkr-contracts = { workspace = true }
   libfuzzer-sys = "0.4"

   [[bin]]
   name = "blueprint_loader"
   path = "fuzz_targets/blueprint_loader.rs"

   [[bin]]
   name = "state_transitions"
   path = "fuzz_targets/state_transitions.rs"
   ```

4. Write `crates/pkr-fuzz/fuzz/fuzz_targets/blueprint_loader.rs`:
   ```rust
   //! Fuzz the blueprint loader. The fuzzer provides random bytes; we
   //! attempt to mmap-load them as a blueprint and exercise the lookup
   //! path. Any panic, abort, or unexpected error is a finding.

   use libfuzzer_sys::fuzz_target;
   use pkr_runtime::mmap::MmapError;
   use std::io::Write;

   fuzz_target!(|data: &[u8]| {
       // Write to a temp file because MmapReader takes a path.
       let path = std::env::temp_dir().join(format!(
           "fuzz-blueprint-{}.bin",
           std::process::id(),
       ));
       if std::fs::write(&path, data).is_err() {
           return;
       }
       // Try to load — most random inputs will fail with InvalidMagic,
       // which is correct behavior. The goal is to catch inputs that
       // pass the magic check but then panic deeper in.
       match pkr_runtime::mmap::MmapReader::new(&path) {
           Ok(r) => {
               let h = pkr_runtime::SolverHandle::new(r);
               // Probe with a few hashes derived from the input.
               for i in 0..data.len().min(8) {
                   let hash = u64::from_le_bytes(data[i..i+8].try_into().unwrap_or([0u8; 8]));
                   let _ = h.get_advice_fast(hash);
               }
           }
           Err(MmapError::InvalidMagic { .. }) => { /* expected */ }
           Err(MmapError::FileTooSmall) => { /* expected */ }
           Err(e) => {
               // Unexpected error type — surface for review.
               eprintln!("FUZZ: unexpected mmap error: {e:?}");
           }
       }
       let _ = std::fs::remove_file(&path);
   });
   ```

5. Write `crates/pkr-fuzz/fuzz/fuzz_targets/state_transitions.rs`:
   ```rust
   //! Fuzz legal-action generation across random game states.

   use libfuzzer_sys::fuzz_target;
   use pkr_core::state::GameState;

   fuzz_target!(|data: &[u8]| {
       let mut state = GameState::new_heads_up();
       let mut buf = [0u8; 16];
       // Treat each byte as an action index, apply until invalid.
       for &a in data.iter().take(50) {
           let n = state.legal_actions_into(&mut buf);
           if n == 0 {
               break;
           }
           let pick = (a as usize) % n;
           let action = buf[pick];
           // Apply the action. If this panics, the fuzzer found a bug.
           // If apply fails, we move on.
           let _ = state.apply(action);
       }
   });
   ```

6. Write `ci/scripts/run-fuzz.sh`:
   ```bash
   #!/usr/bin/env bash
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   cargo install cargo-fuzz 2>&1 | tail -1 || true

   # 5 minutes per target — keeps weekly budget manageable.
   for T in blueprint_loader state_transitions; do
       echo "==> fuzz: $T (5 min)"
       cargo fuzz run "$T" -- -max_total_time=300 --release || {
           # Fuzz crash — save the input.
           echo "FAIL: $T crashed. Artifacts in fuzz/artifacts/"
           mkdir -p fuzz-artifacts/
           cp -r crates/pkr-fuzz/fuzz/artifacts "fuzz-artifacts/$T" 2>/dev/null || true
           exit 1
       }
   done
   ```

7. Add to `weekly.yml`:
   ```yaml
         - uses: taiki-e/install-action@v2
           with:
             tool: cargo-fuzz
         - name: Fuzz (10 min budget)
           run: ./ci/scripts/run-fuzz.sh
         - name: Upload fuzz artifacts on failure
           if: failure()
           uses: actions/upload-artifact@v4
           with:
             name: fuzz-artifacts
             path: fuzz-artifacts/
             retention-days: 90
   ```

8. Commit.

**Acceptance Criteria**
- [ ] Both fuzz targets build and run without crashing on empty input.
- [ ] A 5-min run on each target reports no panics (or finds one, which is even better).
- [ ] On crash, the reproducer input is saved as an artifact.
- [ ] `weekly.yml` wall time including fuzz is < 120 min.

---

## B20 — `miri` run on `pkr-core` + `pkr-cfr` (subset)

**Objective.** Miri catches UB, data races, and pointer-misuse. It's slow — we limit to two crates and only the `lib` targets, not the integration tests. Weekly.

**Exclusive File Paths**
- `ci/scripts/run-miri.sh` (new)

**Dependencies**
- B17.

**Instructions**

1. Write `ci/scripts/run-miri.sh`:
   ```bash
   #!/usr/bin/env bash
   # Run miri on the two crates with most unsafe code.
   # Skips integration tests and binaries — too slow for weekly budget.
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   # Install miri via rustup component. The dtolnay/rust-toolchain action
   # in weekly.yml should already have it if we ask for it.
   rustup component add miri 2>/dev/null || true

   # Scope: pkr-core + pkr-cfr. Both have unsafe atomic code (pkr-cfr
   # metrics.rs) and from_le_bytes (pkr-runtime lookup.rs, not included
   # here for budget reasons).
   for CRATE in pkr-core pkr-cfr; do
       echo "==> miri: $CRATE"
       MIRIFLAGS="-Zmiri-disable-isolation -Zmiri-strict-init" \
           cargo miri test -p "$CRATE" --lib --quiet 2>&1 | tail -30
   done
   ```

2. Add to `weekly.yml`:
   ```yaml
         - uses: dtolnay/rust-toolchain@nightly
         - run: rustup component add miri
         - name: Miri (pkr-core, pkr-cfr)
           run: ./ci/scripts/run-miri.sh
   ```

3. Commit.

**Acceptance Criteria**
- [ ] `cargo miri test -p pkr-core --lib` exits 0 (no UB detected).
- [ ] `cargo miri test -p pkr-cfr --lib` exits 0.
- [ ] Weekly wall time including miri is < 120 min.

---

## B21 — PR-comment diff bot (criterion vs main)

**Objective.** The single most useful artifact for fast iterations: a PR comment that shows the perf delta vs `main` for every micro-bench. Powered by Bencher's PR-comment feature (preferred) or a custom `pull_request` workflow.

**Exclusive File Paths**
- `.github/workflows/pr-comment.yml` (new — only if NOT using Bencher's built-in)

**Dependencies**
- B11.

**Instructions**

### Path A: Bencher's built-in PR comment (recommended)

If B11 path A is done, Bencher's PR-comment feature is already enabled. Add a `pull_request` trigger to `bench.yml` that runs a **subset** (only `pkr-runtime-bench` + `pkr-eval-bench`, the fastest two) and uses Bencher's `--branch "$PR_BRANCH"` to compare against `main`:

1. Modify `bench.yml` to also trigger on `pull_request`:
   ```yaml
   on:
     schedule:
       - cron: '17 3 * * *'
     pull_request:
       paths:
         - 'crates/**'
         - 'benches/**'
         - 'Cargo.toml'
     workflow_dispatch:
   ```

2. Add a job `bench-pr` that runs only the runtime + eval benches (cheap subset) and pushes to Bencher under the PR branch:
   ```yaml
     bench-pr:
       if: github.event_name == 'pull_request'
       runs-on: ubuntu-22.04
       timeout-minutes: 15
       steps:
         - uses: actions/checkout@v4
         - uses: dtolnay/rust-toolchain@stable
         - uses: Swatinem/rust-cache@v2
           with:
             shared-key: bench-release
             cache-targets: true

         - name: Restore smoke artifacts
           uses: actions/cache@v4
           with:
             path: outputs/v0-smoke
             key: smoke-abstractions-${{ runner.os }}-${{ hashFiles('ci/cache-key.sh', 'smoke.sh') }}

         - name: Run subset benches
           run: |
             export PKR_HAND_RANKS=outputs/v0-smoke/hand_ranks.bin
             export PKR_BLUEPRINT=outputs/v0-smoke/blueprint.bin
             cargo bench -p pkr-runtime-bench -p pkr-eval-bench -- \
                 --save-baseline pr --output-format=bencher \
                 > pr-bench.ndjson

         - name: Push to Bencher with PR context
           if: ${{ secrets.BENCHER_API_TOKEN != '' }}
           env:
             PR_NUMBER: ${{ github.event.pull_request.number }}
             PR_BRANCH: ${{ github.event.pull_request.head.ref }}
           run: |
             ./bencher --token "$BENCHER_API_TOKEN" \
                 --project pkr-sota \
                 run --adapter cargo_bench_json --file pr-bench.ndjson \
                 --branch "$PR_BRANCH" \
                 --testbed ci-ubuntu-22.04 \
                 --if-branch-starts-with "$PR_BRANCH/" \
                 --else-if-branch-starts-with "main/" \
                 --else-start-point "${{ github.event.pull_request.base.ref }}"
   ```
   Bencher will auto-comment on the PR with the diff vs the merge-base.

### Path B: Custom PR comment (no Bencher)

1. Write `.github/workflows/pr-comment.yml`:
   ```yaml
   name: pr-comment

   on:
     pull_request:
       paths:
         - 'crates/**'
         - 'benches/**'

   jobs:
     bench-pr:
       runs-on: ubuntu-22.04
       timeout-minutes: 15
       steps:
         - uses: actions/checkout@v4
           with:
             fetch-depth: 0   # need main for diff
         - uses: dtolnay/rust-toolchain@stable
         - uses: Swatinem/rust-cache@v2
           with:
             shared-key: bench-release
             cache-targets: true

         - name: Restore smoke artifacts
           uses: actions/cache@v4
           with:
             path: outputs/v0-smoke
             key: smoke-abstractions-${{ runner.os }}-${{ hashFiles('ci/cache-key.sh', 'smoke.sh') }}

         - name: Run benches on PR branch
           run: |
             export PKR_HAND_RANKS=outputs/v0-smoke/hand_ranks.bin
             export PKR_BLUEPRINT=outputs/v0-smoke/blueprint.bin
             cargo bench -p pkr-runtime-bench -p pkr-eval-bench -- \
                 --save-baseline pr > /dev/null 2>&1

         - name: Checkout main, re-run benches
           run: |
             git stash
             git checkout origin/main
             cargo bench -p pkr-runtime-bench -p pkr-eval-bench -- \
                 --save-baseline main > /dev/null 2>&1
             git checkout -
             git stash pop || true

         - name: Compare baselines
           id: cmp
           run: |
             cargo bench -p pkr-runtime-bench -p pkr-eval-bench -- \
                 --baseline main --noplot 2>&1 \
                 | python3 ci/scripts/extract-criterion-diff.py \
                 > diff.md
             cat diff.md
             echo "body<<EOF" >> $GITHUB_OUTPUT
             cat diff.md >> $GITHUB_OUTPUT
             echo "EOF" >> $GITHUB_OUTPUT

         - name: Post comment
           uses: actions/github-script@v7
           with:
             github-token: ${{ secrets.GITHUB_TOKEN }}
             script: |
               github.rest.issues.createComment({
                 issue_number: context.issue.number,
                 owner: context.repo.owner,
                 repo: context.repo.repo,
                 body: `${{ steps.cmp.outputs.body }}`,
               });
   ```

2. Write `ci/scripts/extract-criterion-diff.py`:
   ```python
   #!/usr/bin/env python3
   """Parse `cargo bench --baseline X --noplot` output into a Markdown
   table. Criterion prints lines like:
       fnv1a/u64_input      12.3 ns/iter ± 0.5  (± 4.1%)  1.05x slower
   """
   import re, sys
   pat = re.compile(
       r"(\S+)\s+([\d.]+\s*\w+/\w+|[\d.]+\s*\w+)\s*±\s*([\d.]+%)\s*"
       r"(?:\(.*?\))?\s*(?:(\d+\.\d+x)\s+(slower|faster))?"
   )
   print("| benchmark | PR | change | verdict |")
   print("|---|---|---|---|")
   for line in sys.stdin:
       m = pat.search(line)
       if not m: continue
       bench, pr_val, pr_ci, ratio, verdict = m.groups()
       if ratio:
           verdict = f"{ratio} {verdict}"
       else:
           verdict = "noise"
       print(f"| `{bench}` | {pr_val} | {pr_ci} | {verdict} |")
   ```

3. Commit.

**Acceptance Criteria**
- [ ] PR comment appears within 15 min of opening a PR that touches `crates/` or `benches/`.
- [ ] Comment contains a table with one row per micro-bench (only the PR-subset: `pkr-runtime-bench`, `pkr-eval-bench`).
- [ ] A > 5% regression is flagged "slower".
- [ ] Comment is updated (not duplicated) when new commits push to the same PR.

---

## B22 — Dashboard README badge row

**Objective.** A single line of badges at the top of `README.md` so anyone glancing at the repo sees the current state of CI + perf.

**Exclusive File Paths**
- `README.md` (modified — add badge row after the title)

**Dependencies**
- B1, B10, B12, B17.

**Instructions**

1. Open `README.md`. Find the title line:
   ```
   # pkr-sota 🃏⚡
   ```

2. Insert directly below:
   ```markdown
   [![fast](https://github.com/<OWNER>/pkr-sota/actions/workflows/fast.yml/badge.svg?branch=main)](https://github.com/<OWNER>/pkr-sota/actions/workflows/fast.yml)
   [![smoke](https://github.com/<OWNER>/pkr-sota/actions/workflows/smoke.yml/badge.svg?branch=main)](https://github.com/<OWNER>/pkr-sota/actions/workflows/smoke.yml)
   [![audit](https://github.com/<OWNER>/pkr-sota/actions/workflows/audit.yml/badge.svg)](https://github.com/<OWNER>/pkr-sota/actions/workflows/audit.yml)
   [![bench](https://github.com/<OWNER>/pkr-sota/actions/workflows/bench.yml/badge.svg)](https://github.com/<OWNER>/pkr-sota/actions/workflows/bench.yml)
   [![proftest](https://github.com/<OWNER>/pkr-sota/actions/workflows/proftest-ci.yml/badge.svg)](https://github.com/<OWNER>/pkr-sota/actions/workflows/proftest-ci.yml)
   [![weekly](https://github.com/<OWNER>/pkr-sota/actions/workflows/weekly.yml/badge.svg)](https://github.com/<OWNER>/pkr-sota/actions/workflows/weekly.yml)
   [![Bencher](https://api.bencher.dev/perf/pkr-sota?branches=main&testbeds=ci-ubuntu-22.04&kinds=latency)](https://bencher.dev/perf/pkr-sota)
   ```

3. Replace `<OWNER>` with the real GitHub owner (e.g. `pkr-sota`).

4. Commit.

**Acceptance Criteria**
- [ ] All 7 badges render on GitHub.
- [ ] Clicking a badge navigates to the workflow's history page.
- [ ] The Bencher badge links to the project's perf dashboard.

---

# TIER 4 — Stretch

## B23 — Flamegraph on proftest, committed as artifact

**Objective.** A weekly flamegraph shows where training spends time. Not a blocking metric; an investigative aid when a regression appears.

**Exclusive File Paths**
- `ci/scripts/run-flamegraph.sh` (new)

**Dependencies**
- B12, B17.

**Instructions**

1. Write `ci/scripts/run-flamegraph.sh`:
   ```bash
   #!/usr/bin/env bash
   set -euo pipefail
   cd "$(dirname "$0")/../.."

   cargo install flamegraph 2>&1 | tail -1 || true

   # 10K iters is enough sample; ~5 sec on a 4-vCPU runner.
   PROF_DIR="${PROF_DIR:-./outputs/v0-flame}"
   ITERATIONS=10000 THREADS=4 CAPACITY=1000000 \
       PROF_DIR="$PROF_DIR" ./proftest.sh &
   PID=$(pgrep -f "target/release/pkr-trainer" | head -1)
   [ -n "$PID" ] || { echo "FAIL: couldn't find trainer PID"; exit 1; }

   sudo flamegraph -o "$PROF_DIR/flame.svg" -p "$PID" -- 15
   wait
   ```

2. Add to `weekly.yml`:
   ```yaml
         - name: Flamegraph
           run: sudo ./ci/scripts/run-flamegraph.sh
         - uses: actions/upload-artifact@v4
           with:
             name: flamegraph
             path: outputs/v0-flame/flame.svg
             retention-days: 30
   ```

3. Commit.

**Acceptance Criteria**
- [ ] `flame.svg` is produced.
- [ ] It opens in a browser and shows hot functions.
- [ ] Weekly wall time including flamegraph is < 150 min.

---

## 5. Appendices

### Appendix A — Worklog Protocol

Each task card finishes by appending to `/home/z/my-project/worklog.md` (or, in the repo, `worklog.md` at the root):

```markdown
---
Task ID: B<n>
Agent: <agent name>
Task: <one-line summary of B<n>>

Work Log:
- <concrete step 1>
- <concrete step 2>
- ...

Stage Summary:
- <key results / decisions / artifacts produced>
```

The worklog is append-only. Never overwrite. New entries go to the bottom.

### Appendix B — Glossary

| Term | Definition |
|---|---|
| **PR gate** | A CI workflow that must pass before a PR can merge. In this plan: `fast.yml`, and `smoke.yml` for PRs touching certain paths. |
| **Nightly** | A CI workflow that runs once per day, on schedule. In this plan: `bench.yml`, `proftest-ci.yml`, `audit.yml`. |
| **Weekly** | A CI workflow that runs once per week, on schedule. In this plan: `weekly.yml` (coverage, dhat, fuzz, miri, flamegraph). |
| **Micro-bench** | A criterion benchmark that isolates a single function or small code path. Run time ≤ 10 seconds. |
| **Integration bench** | A benchmark that exercises the full pipeline or a large chunk of it. Run time ≤ 30 minutes. |
| **Quality metric** | A metric that reflects algorithmic correctness, not speed. In this plan: Kuhn exploitability, NLHE sampled-BR exploitability. |
| **Resource metric** | A metric that reflects resource use, not algorithmic output. In this plan: binary size, compile time, RSS, peak heap, coverage %. |
| **Baseline** | A stored reference result that future runs compare against. In criterion, `--save-baseline <name>`. In Bencher, the historical time series on `main`. |
| **Delta** | The change between two runs. A 5% delta means the new run is 5% slower (or faster). |
| **Bencher** | A benchmarking CI tool that stores historical results, computes statistical significance, and posts PR comments. OSS, self-hostable. |
| **`smoke artifacts`** | The pre-computed `.bin` files in `outputs/v0-smoke/` (`hand_ranks.bin`, `centroids.bin`, `preflop_abstraction.bin`, etc.). Cached across runs to avoid re-computing. |
| **`proftest artifacts`** | The post-training `metrics.csv`, `stats.json`, and `blueprint.bin` in `outputs/v0-proftest*/`. |
| **`perf-history` branch** | A dedicated git branch that holds nightly bench result JSONs as committed files. Fallback for teams that don't want a Bencher server. |

### Appendix C — Anti-Patterns to Avoid

1. **Do not add `#[bench]` to existing `crates/*/src/lib.rs`.** Rust's built-in `#[bench]` is unstable. We use the external `benches/<crate>_bench/` directory pattern, which keeps the source clean and works on stable.
2. **Do not run `cargo nextest --release` in `fast.yml`.** Release tests take 3-5x longer. Debug is fine for the PR gate; release is reserved for `smoke.yml` and `bench.yml`.
3. **Do not enable LTO for bench builds.** LTO changes codegen enough to distort micro-bench results. Add a `[profile.bench]` section to the root `Cargo.toml` that disables LTO for benches:
   ```toml
   [profile.bench]
   opt-level = 3
   lto = "off"
   codegen-units = 1
   debug = false
   ```
4. **Do not commit `Cargo.lock` from a bench crate.** Each `benches/<crate>_bench/` should be in the workspace; commit the root `Cargo.lock` only.
5. **Do not push micro-bench results from a PR branch to Bencher's `main` branch.** That pollutes the baseline. Use `--branch "$PR_BRANCH"` so the PR result is a separate time series.
6. **Do not block PRs on perf regressions < 5%.** Most are noise. Bencher's t-test at p<0.05 will catch real regressions.
7. **Do not run dhat on the `release` profile.** dhat disables LTO via its global-allocator substitution; the resulting binary is not representative of real throughput. Only the memory numbers are valid.
8. **Do not let `weekly.yml` exceed 2 hours.** If a step grows, split it into a separate workflow (`weekly-part-2.yml`).
9. **Do not modify `Cargo.toml`'s `[profile.release]` block.** The current settings (`opt-level=3, lto=fat, codegen-units=1, panic=abort, debug=true, strip=symbols`) are deliberate. Only `[profile.bench]` may be added (per anti-pattern #3).
10. **Do not silently change CSV column order in `binaries/pkr-trainer/src/main.rs`.** `ci/scripts/parse-metrics-csv.py` depends on the existing header. New columns append; existing columns never reorder or rename.

### Appendix D — References

- **Criterion** — https://bheisler.github.io/criterion.rs/book/
- **Bencher** — https://bencher.dev/
- **cargo-nextest** — https://nexte.st/
- **cargo-audit** — https://github.com/RustSec/rustsec
- **cargo-deny** — https://embarkstudios.github.io/cargo-deny/
- **cargo-llvm-cov** — https://github.com/taiki-e/cargo-llvm-cov
- **cargo-fuzz** — https://doc.rust-lang.org/cargo/reference/fuzzing.html
- **Miri** — https://github.com/rust-lang/miri
- **dhat** — https://github.com/nnethercote/dhat-rs
- **flamegraph** — https://github.com/flamegraph-rs/flamegraph
- **Swatinem/rust-cache** — https://github.com/Swatinem/rust-cache
- **dtolnay/rust-toolchain** — https://github.com/dtolnay/rust-toolchain
- **taiki-e/install-action** — https://github.com/taiki-e/install-action
- **External CFR literature** (for context, not this plan): Lanctot 2009, Tammelin 2014, Brown & Sandholm 2019, Farina et al. 2021 — see `docs/SOTA_UPGRADE_GUIDE_pkr-sota.md` §1.

### Appendix E — Mapping Back to Existing `docs/SOTA_UPGRADE_GUIDE_pkr-sota.md`

This instrumentation plan is **orthogonal** to the algorithmic upgrade tasks (T1-T15) in the SOTA guide. They compose:

| SOTA guide task | This plan's instrumentation that verifies it |
|---|---|
| T1 (unbias MCCFR estimator) | B14 Kuhn exploitability (convergence speed) + B15 NLHE sampled-BR |
| T2 (mask illegal actions) | B14 (Kuhn unchanged) + B13 `stats.json` strategy analysis (entropy hist, dominant counts) |
| T3 (fix Kuhn harness) | B14 depends on T3 — Kuhn must be correct before B14 trends are meaningful |
| T5 (alternating CFR+ updates) | B14 + B15 |
| T6 (street-scoped infoset keys) | B5 (fnv1a benchmark for new key size) + B12 (re-train cost) |
| T8 (sampled abstract-game exploitability) | B15 IS T8's measurement infrastructure |
| T11 (wire action translation) | B7 (runtime lookup bench must show no regression) |
| T12 (Eytzinger lookup) | B7 — direct comparison: binary search vs Eytzinger |
| T14 (real river re-solve) | B15 (new "post-resolve" exploitability metric) + B12 (re-solve cost in wall time) |

If you are executing both guides simultaneously, **do T1-T3 before B14/B15** (otherwise you'll trend a biased estimator), and **do B1-B4 before any SOTA tier-1 task** (otherwise you can't verify any change).

---

## 6. Rollout Schedule (Suggested, Not Binding)

| Week | Tasks | Outcome |
|---|---|---|
| W1 | B1, B2, B3 | PR gate, smoke gate, supply-chain gate all live. Devs see green checks within 4 minutes of pushing. |
| W2 | B4, B5, B6, B7 | First 4 micro-bench crates exist and run locally. |
| W3 | B8, B9, B10, B11 | All 6 micro-bench crates wired into nightly CI; Bencher dashboard live. |
| W4 | B12, B13, B14, B15 | Throughput + exploitability trending on Bencher. PR comments appear. |
| W5 | B16, B17, B18 | Binary size, compile time, coverage, memory all trended. |
| W6 | B19, B20, B21, B22 | Fuzz + miri wired; PR-comment bot live; README badges live. |
| W7 | B23 | Flamegraph weekly. |

**Total calendar time to "fully instrumented": ~6 weeks of one-engineer effort.** Each individual task card is < 1 day of work; the elapsed time is dominated by review and CI-debugging cycles.

---

## 7. Closing Notes

- **Fast iteration is preserved by tier separation.** The 4-minute PR gate (B1) is the only thing devs wait on during normal work. Everything else is async — nightly, weekly, or PR-comment-bot.
- **Every metric is a number on a dashboard.** No more "feels slower" or "I think the bug is in the flush path." Look at Bencher; look at the PR comment; look at the flamegraph.
- **The dumb agent instructions are the task cards.** They are self-contained, name exact file paths, give exact code, and name the acceptance criteria. No card depends on the agent inferring context from anywhere except `worklog.md` and the real source files (rule 7).
- **Nothing here changes the algorithm.** The CFR code, the DCFR math, the regret table layout — all untouched. This plan adds observability, not behavior. The only behavior changes are: (a) `proftest.sh` runs the eval loop by default (B15), (b) `binaries/pkr-trainer` gains an opt-in `dhat-profiling` feature (B18). Both are additive.
- **The instrumented codebase is the prerequisite for the algorithmic upgrades in `docs/SOTA_UPGRADE_GUIDE_pkr-sota.md`.** Without B14/B15, T1/T2/T5/T8 ship unverified. Without B7, T12 (Eytzinger lookup) is unverifiable. Without B12, T14 (river re-solve) is unverifiable. **Do this plan first, then execute the SOTA guide.**

---

*End of plan.*
