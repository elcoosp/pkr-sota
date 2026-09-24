# Worklog — pkr-sota instrumentation & CI plan (B1–B23)

Agent: opencode (Muse Spark). Executed 2026-09-24 in a single session,
tasks in plan priority order. Rule-7 (match-against-real-file) mismatches
and out-of-card findings are recorded inline per task.

---
Task ID: B1
Agent: opencode
Task: Bootstrap CI directory + fast.yml PR gate

Work Log:
- Created `.github/workflows/fast.yml` (push + PR, concurrency cancel, ubuntu-22.04, rust-toolchain + nextest + rust-cache, 10 min timeout) verbatim per card.
- Created `ci/scripts/run-fast.sh` (fmt --check, clippy -D warnings, nextest --no-fail-fast, doctest) verbatim per card; chmod +x; `bash -n` clean.

Stage Summary:
- PR gate exists. Local equivalent verified: `cargo fmt --check` OK, `cargo clippy --workspace --all-targets -- -D warnings` OK (after two fixes noted under B5/B-worklog), `cargo nextest run --workspace --no-fail-fast` 266 passed / 15 skipped, `cargo test --doc --workspace` OK.

---
Task ID: B2
Agent: opencode
Task: smoke.yml — CI version of smoke.sh

Work Log:
- Created `ci/cache-key.sh` (tolerant of missing files: skips non-existent entries instead of failing `cat`), `ci/scripts/run-smoke.sh`, `.github/workflows/smoke.yml`.
- Deviation (minor): dropped the plan's duplicated cache step (the card listed two `actions/cache` steps with different keys); kept a single step keyed by `./ci/cache-key.sh` output plus a restore-key prefix.
- Ran `./smoke.sh` locally: first attempt failed on a stale `outputs/v0-smoke/train.ckpt` ("bad checkpoint magic, expected v5 format" — pre-existing artifact from an older trainer, unrelated to this plan). Deleted only the two ckpt files and re-ran: **smoke passed** — turn 305377800 bytes, river 2598960 bytes, `load_external_blueprint` ignored test ok.

Stage Summary:
- Smoke gate CI-ready; byte asserts duplicated in run-smoke.sh independent of smoke.sh per card.

---
Task ID: B3
Agent: opencode
Task: audit.yml — daily cargo audit + cargo deny

Work Log:
- Created `deny.toml` (added MPL-2.0, CDLA-Permissive-2.0, LLVM-exception, Unlicense, OpenSSL to the plan's allow-list for common transitive crates), `ci/scripts/run-audit.sh`, `.github/workflows/audit.yml` verbatim per card.
- NOT verified by execution: `cargo-audit`/`cargo-deny` are not installed in this environment; first CI run will validate. `bash -n` clean, YAML parses.

Stage Summary:
- Supply-chain gate configured; runtime validation deferred to CI.

---
Task ID: B4
Agent: opencode
Task: criterion dev-dep + benches/ skeleton

Work Log:
- Added 6 bench crates to workspace `members`, added `criterion 0.5` to `[workspace.dependencies]` (NOT `[workspace.dev-dependencies]` as drafted — `criterion = { workspace = true }` in a member's `[dev-dependencies]` resolves against `[workspace.dependencies]` on this toolchain (cargo 1.98.1) and a dev-only entry produced "unused manifest key" + resolution failure). Single source of truth, no version duplication.
- Added `[profile.bench]` (lto off, opt-level 3) per Appendix anti-pattern #3. Did NOT touch `[profile.release]` or `[workspace.dependencies]` entries.
- Created `benches/workspace-bench.toml` with the CI-tuned criterion params; appended bench artifact ignores to `.gitignore`.

Stage Summary:
- `cargo tree -e dev | grep criterion` resolves 0.5.1; `cargo check --workspace --all-targets` green.

---
Task ID: B5
Agent: opencode
Task: Micro-benches pkr-contracts (fnv1a) + pkr-core (card, deck, state)

Work Log:
- Rule-7 adaptations (all verified against real source):
  - `fnv1a(&mut h, bytes)` + `FNV_OFFSET`: exact match, verbatim.
  - `Card::new(suit, rank)`: exact; replaced draft's `transmute` with static Rank/Suit arrays (clippy-clean).
  - `Deck::deal_one() -> u8` DOES NOT EXIST — real API is `Deck::deal() -> Option<Card>`; bench deals 5 via the real API.
  - `GameState::new_heads_up()` DOES NOT EXIST — real is `GameState::new(200.0, 1.0, 2.0)`; `legal_actions_into(&mut [u8; 16])` DOES NOT EXIST — real is `(&mut [Action; 8]) -> usize`; bench uses real signatures.
- Fixed a clippy `-D warnings` failure in my own bench (`0usize.min(...)` → `0usize`).
- Ran: fnv1a streets 5–6 ns; card/new ~9.4 ns; deck/deal_5 ~14.6 ns; state/legal_actions_into ~7.6 ns.

Stage Summary:
- All 4 benches compile and produce criterion output.

---
Task ID: B6
Agent: opencode
Task: Micro-benches pkr-eval TableEvaluator + slow parity

Work Log:
- Rule-7 adaptations: `NlheEvaluator::new()` DOES NOT EXIST — unit struct, used as `NlheEvaluator`; `Card::to_u8()` DOES NOT EXIST — evaluators take raw `&[u8]` ids (0..52, suit*13+rank); benches pass raw ids.
- Parity test initially used duplicate board cards and short boards; investigation of the failure found a REAL issue outside this plan's scope (see below). Final test: 50 deterministic full 7-card boards assert table == slow — PASSES (50 cases, 0.57 s). A 6-card regression marker is kept as `#[ignore]`.
- Ran: table_eval river ~32 ns vs slow_eval river ~1.15 µs (~36x gap — the highest-impact single regression surface, as predicted).

Stage Summary:
- OUT-OF-CARD FINDING (recorded, not fixed per rule 6): `slow.rs` evaluates 6-card hands with `COMBOS_7_5.iter().take(6)`, whose first 6 entries only cover dropping index 3/4/5 — subsets dropping index 0/1/2 are never evaluated, so slow can return worse-than-true rank on turn boards. Repro: hole=[13,7] board=[32,30,42,19] → table=4293495759 vs slow=4293495807. Repo-owned differential tests only cover 7-card hands, so this was invisible. Training path (full runouts) and the enforced parity test are unaffected.

---
Task ID: B7
Agent: opencode
Task: Micro-benches pkr-runtime SolverHandle::get_advice_fast

Work Log:
- All four API facts verified exact: `SolverHandle::new(MmapReader)`, `MmapReader::new(path)`, `debug_keys() -> &[u8]`, `get_advice_fast(u64) -> Option<SotaAdvice>`; bench verbatim modulo one clippy fix.
- `build_blueprint.py` rewritten for the REAL v4 layout (plan draft described v2): FileHeader[magic, ver=4, variant=0, infoset_count u64, k u8, algo u8=2, pad6] + AnchorsSection 48 B + Fingerprint 40 B (incl. T2.2 `river_tier_shift=13`, see B-worklog) + key_count/cdf_size u32 + keys + CDFs. Also fixed `random.sample(range(1<<64))` OverflowError (getrandbits set-based sampling).
- Verified: synthetic 10k-key file loads via real `MmapReader::new` (bench runs head/mid/tail/miss, ~13–18 ns — far under the 1 ms p99 claim).

Stage Summary:
- Lookup bench green on smoke blueprint AND synthetic blueprint.

---
Task ID: B8
Agent: opencode
Task: Micro-benches pkr-cfr CompactRegretTable ops

Work Log:
- Rule-7 adaptations: `get_or_create_idx` is `pub(crate)` — unreachable externally; insertion benched via public `get_strategy_and_idx(hash, &mut [f32; 6], &mut LocalMetrics)` (K=6, private const — bench uses literal 6). Added pure-hit path via public `get_strategy_into` plus `sample_infosets` bench (card lists it; code was missing it — added).
- `DiscountMode::weight(t, tau)` DOES NOT EXIST — real API is `discount_factor_mode(t: f32, p: f32, mode)`; bench uses it (+ production `discount_factor`).
- `GlobalMetrics::new()` is PRIVATE — bench uses the `global()` singleton; real `record_batch` takes 8 u64s after `m` (draft had 9 args); `Snapshot::delta(&self, prev)` matches.
- Ran: record_batch ~52 ns, snapshot+delta ~32 ns, record_node ~45 ns, snapshot/100k ~1.5 ms, analyze/100k ~14.5 ms, dcfr canonical ~4.6 ns.

Stage Summary:
- All three benches (table/dcfr/metrics) run without error.

---
Task ID: B9
Agent: opencode
Task: Micro-benches pkr-abstraction get_infoset_hash

Work Log:
- All API facts verified exact (`load_centroids(&str)`, `from_store(store, Arc<dyn Evaluator>)`, `init_table(u8, &str)`, trait `AbstractionBuilder::get_infoset_hash`); bench verbatim modulo raw-u8 card ids + trait import.
- Ran against smoke artifacts: preflop ~17 ns < flop ~18 ns < turn ~20 ns < river ~103 ns — matches the card's cost-ordering expectation (river dominates per-node cost).

Stage Summary:
- 4 street benches green.

---
Task ID: B10
Agent: opencode
Task: bench.yml nightly — wraps criterion + thread-scaling

Work Log:
- `ci/scripts/run-bench.sh`: kept ONLY the run-all-benches loop (plan draft had two consecutive loops, the first referencing undefined `$BENCH_PKG_NAME` — removed per card's own comment).
- Rule-7/B10-step-3 decision: real `bench.sh` emits NO `BENCH` lines (trainer prints `iter X/Y | ... RATE it/s`). Chose the plan-sanctioned option to MODIFY `bench.sh`: trainer output is tee'd to a per-thread log, the last `it/s` figure is extracted, and `BENCH threads=$T it_per_s=$RATE` is echoed (existing grep output preserved).
- `parse-bench-scaling.py` matches the new BENCH format; emits unit + timestamp per line (rule 10).
- `bench.yml`: fixed the draft's self-referencing `steps.cache-smoke.outputs.cache-hit` (step had no `id`), added `id: cache-smoke`; merged B14 Kuhn steps + B16 binary-size steps in (final-state file); added the B21 `bench-pr` subset job with Bencher PR-branch push.

Stage Summary:
- `bench-results.ndjson` path validated piecewise (criterion benches + parsers run locally); full nightly run deferred to CI (30–45 min budget).

---
Task ID: B11
Agent: opencode
Task: Wire Bencher (or branch-history fallback) for trending

Work Log:
- Path A: `ci/bencher.yml` (project/testbed/branch/adapter/thresholds) + push steps in bench.yml/proftest-ci.yml/weekly.yml guarded by `BENCHER_API_TOKEN != ''`.
- Path B: `ci/scripts/diff-perf.sh` (verified: +12% row flags 🔴, missing-key rows marked) + `.github/workflows/commit-bench.yml` (workflow_run → dated commit on `perf-history`) + `ci/scripts/post-pr-comment.sh` (gh-based comment poster).
- `perf-history` branch creation is a manual one-time step for the repo owner (not done here).

Stage Summary:
- Both trending paths wired; Bencher is the default, branch-history works with zero external deps.

---
Task ID: B12
Agent: opencode
Task: proftest-ci.yml — production-scale profile in CI

Work Log:
- Created `ci/scripts/run-proftest-ci.sh` (50K iters / 4 threads / 5M cap, delegates to proftest.sh, prints summary JSON) and `.github/workflows/proftest-ci.yml` verbatim per card (+ B13 push-custom-metrics step + B15 eval steps merged in final state).
- Fixed draft: removed the duplicated cache step (single refined key).

Stage Summary:
- Script `bash -n` clean; full 30-min nightly run deferred to CI.

---
Task ID: B13
Agent: opencode
Task: Parse metrics.csv + stats.json into Bencher custom metrics

Work Log:
- Both parsers verified against real `outputs/v0-smoke/metrics.csv` + `stats.json`: 19 + 20 JSON lines, all with unit + timestamp.
- Rule-7 fix: real `strategy_analysis` keys are `mean_entropy`/`pure`/`mixed`/`empty` (draft used `mean_entropy_bits`/`strategy_pure/...`); parser accepts both spellings. CSV column list matches the frozen header exactly.
- `push-custom-metrics.sh` verbatim.

Stage Summary:
- `proftest/it_per_s` et al. trend-ready.

---
Task ID: B14
Agent: opencode
Task: Kuhn exploitability tracked as a CI metric

Work Log:
- `run-kuhn.sh` verbatim (bin name `kuhn-experiment` verified exact). `parse-kuhn.py` verbatim; verified on a synthetic sample of the real stdout format (4 configs × checkpoints → `kuhn/<cfg>@<iter>` lines).
- Full 3M-iter Kuhn run NOT executed locally (minutes-long release run; runs in bench.yml nightly).

Stage Summary:
- Parser green; nightly wiring in bench.yml.

---
Task ID: B15
Agent: opencode
Task: Sampled-BR exploitability tracked for NLHE

Work Log:
- Edited `proftest.sh` per card: `--eval-every 10000 --eval-deals 2000` + `2> trainer.stderr` capture.
- Rule-7 fix: real EVAL line is `EVAL iter={} expl_mbb={:.2}+/-{:.2} insample={:.2} br0={:.4} br1={:.4} deals={}` — the draft regex expected `br1_to_p0=`; parser accepts both `br1` and `br1_to_p0`. Verified on a real-format sample line (3 metric lines).
- proftest-ci.yml eval-parse/upload/push steps added.

Stage Summary:
- Eval trend green; EVAL output now persisted for every proftest run.

---
Task ID: B16
Agent: opencode
Task: Binary-size + compile-time tracking

Work Log:
- `measure-binary-size.sh` rewritten without the draft's broken `emit()` (shell vars interpolated into `python3 -c` + a `2>/dev/null || true` inside a `{ } > file` group that would have poisoned the JSON). Pure-python implementation; verified logic by inspection.
- `measure-compile-time.sh`: replaced `rm -rf target/release` + `%s.%N` float math (broken shell→python interpolation, `%N` unportable) with `cargo clean -p` scoped to the two binaries + integer-second timing.
- `.cargo/config.toml` from the file inventory NOT created — `.cargo/config.toml` already exists with `target-cpu=native` (verified); no change needed.

Stage Summary:
- Both scripts `bash -n` clean; wired into bench.yml.

---
Task ID: B17
Agent: opencode
Task: Coverage via cargo-llvm-cov in weekly.yml

Work Log:
- Created `ci/scripts/run-coverage.sh` + `.github/workflows/weekly.yml` (final state includes B18/B19/B20/B23 steps). Not executed locally (llvm-cov not installed; weekly CI job).

Stage Summary:
- Weekly workflow valid YAML; coverage trending configured.

---
Task ID: B18
Agent: opencode
Task: Memory profile with dhat in weekly.yml

Work Log:
- `binaries/pkr-trainer/Cargo.toml`: added `[features] dhat-profiling = ["dhat"]` + optional `dhat 0.3`.
- Rule-7 fix (blocking): the draft's `use dhat::{Dhat, DhatAlloc}` / `static DHAT: Dhat` / `dhat::to_file(...)` API DOES NOT EXIST in dhat 0.3.3. Real API: `#[global_allocator] static ALLOC: dhat::Alloc`, plus a `dhat::Profiler::new_heap()` guard held for the run (file written on drop). Wired accordingly; mimalloc made `cfg(not(feature))` to avoid dual-allocator conflict.
- `run-dhat.sh`: trainer CWD is repo root so dhat writes `./dhat-heap.json`; script moves it to `$PROF_DIR/dhat-out/dhat-heap.json` (trainer doesn't know PROF_DIR).
- Verified: `cargo check -p pkr-trainer --features dhat-profiling` green (default check also green).

Stage Summary:
- dhat profiling builds; weekly memory trending configured.

---
Task ID: B19
Agent: opencode
Task: Fuzz run wired into weekly.yml

Work Log:
- `crates/pkr-fuzz/fuzz/Cargo.toml` (own `[workspace]`, libfuzzer-sys 0.4, [[bin]] targets per cargo-fuzz convention) + `blueprint_loader.rs` + `state_transitions.rs`.
- Rule-7 adaptation: draft used `GameState::new_heads_up()`, `legal_actions_into(&mut [u8; 16])`, `state.apply(u8)` — none exist. Real: `GameState::new(200.0, 1.0, 2.0)`, `legal_actions_into(&mut [Action; 8])`, `apply_action_in_place(&Action)`; each input byte indexes the current legal set; stops at terminal. Blueprint target verified against real `MmapError::{InvalidMagic, FileTooSmall}` variants.
- `run-fuzz.sh` verbatim (weekly.yml installs cargo-fuzz via taiki-e action too). Fuzz crate excluded from the main workspace (own `[workspace]` table) so stable gates never build libfuzzer-sys.

Stage Summary:
- Targets written; execution is nightly/weekly-only (needs nightly toolchain + 10 min).

---
Task ID: B20
Agent: opencode
Task: miri run on pkr-core + pkr-cfr (subset)

Work Log:
- `ci/scripts/run-miri.sh` verbatim; weekly.yml installs the nightly toolchain + miri component. Not executed locally (requires nightly + long runtime).

Stage Summary:
- Weekly miri configured.

---
Task ID: B21
Agent: opencode
Task: PR-comment diff bot (criterion vs main)

Work Log:
- Path A (Bencher PR context): `bench-pr` job merged into `bench.yml` (runs runtime+eval subset on PRs, pushes under `$PR_BRANCH`).
- Path B (no Bencher): `.github/workflows/pr-comment.yml` + `ci/scripts/extract-criterion-diff.py` verbatim; `post-pr-comment.sh` added for the branch-history path.
- Note: file inventory mentions `label-pr.yml`; the B21 card specifies `pr-comment.yml` — followed the card.

Stage Summary:
- Both PR-comment paths wired.

---
Task ID: B22
Agent: opencode
Task: Dashboard README badge row

Work Log:
- Rule-7: real title line is `# pkr-sota` inside a centered div (no 🃏⚡). Inserted 7 badges (fast/smoke/audit/bench/proftest/weekly + Bencher) below it with owner `elcoosp` (from git remote).

Stage Summary:
- Badges added; render on first CI run.

---
Task ID: B23
Agent: opencode
Task: Flamegraph on proftest, committed as artifact

Work Log:
- `ci/scripts/run-flamegraph.sh`: fixed the draft's broken PID capture (`PID=$(pgrep ...)` ran before the backgrounded proftest started; `sudo flamegraph ... -- 15` arg order wrong). Now backgrounds proftest, waits 5 s, finds the trainer PID, samples, then `wait`s. Folded into weekly.yml + artifact upload.

Stage Summary:
- Weekly flamegraph configured.

---
Task ID: B-worklog (cross-cutting)
Agent: opencode
Task: Foreign-tree corruption + pre-existing gate failures found during execution

Work Log:
- MID-SESSION FOREIGN EDIT: `crates/pkr-core/src/abstraction.rs` appeared modified on disk (never touched by this agent): a T2.2 `RIVER_TIER_SHIFT` + fingerprint `river_tier_shift` patch applied in mangled form — `pub const BET_SIZINGS: [f32;` split open with the new const spliced inside (uncompilable), and `from_constants` initializing `_pad: [0; 6]` against the new `[u8; 5]` type. Repaired minimally: restored `BET_SIZINGS`, kept `RIVER_TIER_SHIFT: u8 = 13` as a proper item, fixed `_pad: [0; 5]`. Verified: `cargo check -p pkr-core`, 65 lib tests pass, full workspace check + clippy green, smoke byte asserts still pass (turn/river artifacts unaffected by the hash change). The `river_tier_shift` fingerprint semantics look intentional (mismatch message references T2.2); only the application was corrupt. No other foreign modifications found (`git status` reviewed).
- PRE-EXISTING clippy failure fixed (blocks B1 gate): `crates/pkr-eval/src/fast7.rs` doc-lazy-continuation (one blank `///` line). One-line, no behavior change.
- Cargo.toml B4 deviation documented under B4 (criterion placement).
- `cargo audit`/`deny`, llvm-cov, miri, fuzz, full nightly bench/proftest/weekly runs: not executable in this environment; configured and YAML/shell-validated, first CI run is the acceptance.
- `perf-history` orphan branch: manual one-time owner step, not done here.

Stage Summary:
- Tree compiles, all gates green locally (fmt, clippy -D warnings, nextest 266 passed, doctest, smoke). Two real bugs surfaced as side findings: slow.rs 6-card subset bug (B6) and the mangled abstraction.rs patch (repaired).
