# Changelog

All notable changes to pkr-sota will be documented here.
Verified numbers and their measurement context live in
[`docs/STATUS.md`](docs/STATUS.md) and [`docs/HISTORY.md`](docs/HISTORY.md).

## Unreleased — 2026-11-03 (docs synthesis + full re-audit)

### Changed
- **Documentation synthesized.** 130+ scattered and partially stale doc
  files (experiments, handoffs, roadmap, reviews, playbooks, specs,
  per-run notes) consolidated into three living files:
  `docs/ARCHITECTURE.md` (as-implemented architecture, corrected:
  blueprint format v4, `RIVER_TIER_SHIFT = 15`, real `pkr-runtime`
  dependency set, `--iters-per-sync` 2048), `docs/STATUS.md` (current
  state, verified numbers, honesty rules, closed audit), and
  `docs/HISTORY.md` (session timeline, verified findings incl. the K=6
  era, negative-results register, blueprint compatibility, methodology
  lessons). `README.md` and this changelog refreshed. All superseded
  docs removed; `logo.png` kept.
- Stale claims fixed in the surviving docs: the misfiled foreign
  `HANDOFF.md` (belonged to an unrelated UI project) is gone; the
  "PID 59819 running / decision pending" status lines are gone (K=7
  shipped, run 1 negative, no background jobs); the T2.2 entry below is
  now marked reverted.

### Fixed (full re-audit; TDD — every fix has a regression test)

A five-report, subsystem-by-subsystem audit of the whole workspace
(game rules, eval/abstraction, CFR/export, runtime/subgame,
exploit/fuzz/trainer/CI) produced ~50 confirmed findings; the ones that
could be fixed without changing the training numerics (and thus the
golden hashes) are all landed below. Known gaps that would require a
fingerprint/golden regeneration are listed at the end.

Game model (`pkr-core`, `pkr-contracts`):
- A short all-in while FACING a bet is no longer also offered as a
  spurious Bet/jam (it moves the same chips as Call but mapped to
  JAM_BUCKET, splitting one decision across two abstract actions). The
  M-1 guard used to live only in the branch where it was provably dead.
- Uncalled-bet excess is now returned to the bettor at showdown
  (`terminal_payoff`), with the same correction mirrored in the
  differential-fuzz reference and the BR fast path. Latent today (the
  single-start-stack API keeps investments matched) but real for any
  asymmetric-stack caller.
- `GameState::new` rejects `start_stack < bb` in release builds too.
- Strict min-raise bookkeeping extracted into pure, unit-tested
  functions (`strict_raise_bookkeeping`, `min_raise_to_for`).
- FNV-1a known-answer vectors + streaming/order tests (pkr-contracts
  had zero tests).
- Two test modules compiled into release builds (`#[cfg(test)]` was
  missing); pre-existing clippy violations fixed.

Blueprint I/O (`pkr-export`):
- The v2 acceptance path read `key_count` from inside the keys array:
  the 64-byte AnchorsSection is v3+ only (the runtime mmap reader
  already gated this). A hand-built v2 blueprint regression test pins
  the v2 offsets.
- The opt-in FMph tail is now byte-deterministic for a fixed key set
  (the search RNG is seeded from the key set instead of system
  randomness) and the displacement cap cannot wrap to a smaller search
  space (`max_displacement_for`, u64-saturating).
- Blueprint/checkpoint renames are followed by a parent-directory
  fsync so the rename itself is durable, not just the file content.

CFR (`pkr-cfr`):
- The F5 sequential-fold regression test ran ONLY with an env var set
  even though sequential folding is the production default — rewritten
  to drive both modes explicitly and pin the clamp semantics.
- `GlobalMetrics.iterations` no longer double-counts under
  `PKR_ALT_UPDATES=1` (stats.json / metrics.csv it/s were 2x wrong).
- The dcfr negative floor now uses `R_MAX` instead of a stale
  half-headroom constant.
- `ValueNet::load` returns errors for short/unaligned files instead of
  panicking; `apply_strategy_batch` filters NaN probabilities.

Abstraction/eval (`pkr-abstraction`, `pkr-eval`):
- `init_table` validates the exact per-street table size — a truncated
  or width-mismatched table is rejected loudly instead of degrading
  every lookup on that street into silent Monte-Carlo fallbacks.
- River bucket generation is capped at k <= 256 (the infoset hash
  truncates the board bucket to 8 bits; k=512 under PKR_BUCKET_U16
  aliased boards modulo 256). The three u8-storage generators cap at
  k <= 255 regardless of PKR_BUCKET_U16 (they wrote u8 ids while
  asserting a u16 range).
- `mean_within_cluster_emd` compares CDFs once (it was computing
  CDF-of-CDF on both sides); a point now scores EMD 0 against its own
  centroid.
- `EHS_SAMPLES=0` clamps to 1 (NaN EHS poisoned k-means bucket ids);
  duplicate hole cards fail loudly; fast7 mirrors slow's 7-card assert.

Trainer (`pkr-trainer`):
- The promote-gate re-seed resolves the derived
  `<output_dir>/exploitability.csv` when `--exploitability-csv` is not
  passed — a resumed run without the flag used to auto-promote its
  first eval regardless of the historical best.
- The end-of-run export no longer clobbers a previously promoted
  blueprint when every eval of a resumed run was gate-rejected.
- `--eval-every` is no longer quantized to `--report-every` boundaries
  (evals ran ~10x less often than requested with the defaults' ratio).
- `--eval-mode` is whitelisted (`insample`/`heldout`); typos fail loud.
- Stop reasons (capacity / signal / plateau) are reported in the log.

Runtime/search (`pkr-runtime`, `pkr-subgame`):
- `PKR_VSEARCH_GADGET=1` actually runs the CFR-D gadget solver — it
  silently ran the raw, unbounded solve before.
- The M-4.3 trace buckets use the chips-behind convention, fixing the
  jam misclassification that silently skipped own-range updates in
  classic jam-or-fold spots.
- An opponent all-in no longer applies a phantom Fold to the shadow
  state and tracker under `PKR_SHADOW=1` (falls back to Call when
  facing, Check otherwise).
- `pkr-bot-pipe` validates the request JSON before constructing any
  state (the colliding placeholder hole cards panicked in debug
  builds), and rounds fractional bet totals instead of truncating.
- `MmapReader` warns loudly on an unsorted key table (silent misses
  stay safe but are now visible) and `HealthReport` surfaces miss
  counts; out-of-order observed actions are warn-and-dropped; the
  session search path masks our hole cards from the opponent range
  like `VSearchHandle::decide` does; a stale `#[ignore]`d test was
  rewritten to the current mirror contract.

Exploitability/fuzz (`pkr-exploit`, `pkr-fuzz`):
- Eval deal-seed streams decorrelated (the `(seed+i)*PHI+1` scheme made
  adjacent eval seeds share 999/1000 deals; the eval_paired decision
  seed of hand i collided with hand i+1's deal seed — the exact
  shifted-by-one class that produced the historical correlated-seeds
  incident). Provably disjoint now via a bijective mix plus domain
  bits, pinned by tests.
- `lbr_choose` uses common random numbers across candidates (each
  candidate was scored on different deals — argmax optimism bias);
  the numeric-miss fallback no longer samples a hardcoded combo that
  can be blocked.
- The heldout metric's in-sample companion is measured on the SAME fit
  deals (it used a third stream, conflating deal-set change with
  generalization); `lbr_eval --sig` sets all three sig gates in every
  arm (a parent-exported PKR_SIG_V5 silently overrode --sig 1).
- The public-tree walker now hashes infosets through the production
  env-gated signature path (it fed the v1 4-byte form while training
  emits 8-byte v3 — every lookup missed), pinned by a parity test
  against `GameState::infoset_signature_into`; the known-incorrect BR
  entry points carry deprecations.
- Incomplete hands (step-cap/runout exhaustion) are excluded from
  tournament/paired means and counted in `hands_incomplete` instead of
  contributing phantom 0.0 chips.
- The dead legacy `run_fuzz` (drew board cards with replacement) is
  deleted.

Tooling/CI (shell, workflows, python):
- `run.sh`: the SIGINT/SIGTERM trap actually forwards to the trainer's
  process group, and a same-VERSION concurrent run is blocked by a
  lock instead of interleaving checkpoint writes.
- `verify_artifacts.sh` asserts exact byte sizes for every precompute
  artifact (a truncated table used to be cached forever);
  `run-dhat.sh` passes `--features dhat-profiling` through to the
  profiled run (the weekly job failed every run before);
  `extract-criterion-diff.py` parses Criterion 0.5 output (the PR
  perf-diff table was always empty); `parse-eval-stderr.py` handles
  `EVAL-HELDOUT` lines; `run-coverage.sh` runs the suite once.
- bench-pr/golden workflows share the canonical smoke cache key (they
  could never hit it) and generate on miss; commit-bench has a
  least-privilege permissions block and tolerates a missing
  perf-history branch.
- `slumbot_client.py` now speaks the `pkr-bot-pipe` contract (full
  actions history, winnings forwarding, error_msg abort) with a
  `--self-test`; the wire format against the real Slumbot API remains
  UNVERIFIED (Phase 6).
- `bucket-membership.sh` uses the canonical `suit*13+rank` encoding
  (every printed membership was for the wrong combo); smoke cleans all
  run artifacts between runs; misc portability fixes.

Known gaps deliberately NOT fixed (each would change blueprint bytes
or the abstract game, invalidating golden hashes without a
regeneration pass): `AbstractionFingerprint` does not bind
`EHS_SAMPLES`/`BET_SIZINGS[3]`/`NUM_ACTION_BUCKETS`; k-means build
assigns by L2 while runtime lookup uses L1 (can disagree on boundary
histograms); the epsilon-uniform opponent sampling lacks an importance
correction (documented-intentional); the blend-back loop re-arms a
fresh budget (worst case ~2x `PKR_VSEARCH_BUDGET_MS`).

### Removed
- Superseded docs: `docs/INDEX.md`, `docs/status.md`,
  `docs/arch-overview.md`, `docs/blueprint-compat.md`,
  `docs/HANDOFF.md`, `docs/PKR_AUDIT_AND_FIX_PLAN.md`, `docs/spec/`,
  `docs/experiments/`, `docs/handoffs/`, `docs/roadmap/`,
  `docs/reviews/`, `docs/playbooks/`, `docs/archive/`, `worklog.md`,
  `scripts/T22-*.md`, `scripts/analysis/suit_iso_port_plan.md`,
  `patches/T22-river-resolution.patch` (patch for a REVERTED change),
  and the orphaned `benches/workspace-bench.toml`.

## 2026-10-02 → 2026-10-12 — sessions 4–9 (search, K=7, audit closed)

Summary of the four handoff cycles; details now live in
`docs/HISTORY.md`.

### Added
- **K=7 action abstraction shipped** (2026-10-10): `NUM_ACTION_BUCKETS`
  6 → 7, `BET_SIZINGS` [0.5, 1, 2] → [0.5, 1, 2, 3], bucket 6 = jam.
  Checkpoint-invalidating by design. Baseline: 100M iters, seed 42,
  LBR −162.79 ± 104.49 mbb; H2H search-vs-blueprint +0.958 (t = 5.28) /
  +1.279 (t = 7.02). K=7 is the shipping config.
- **River-only safe search** shipped in two proven modes: the heuristic
  BOUND (`--search-bound`; `final_br ≤ blueprint_br`) and the CFR-D
  gadget (terminate option pays blueprint continuation value by EV).
  Deadline enforced inside BR walks.
- **M-3 strict NLHE min-raise, opt-in** (`PKR_STRICT_MIN_RAISE=1`):
  correct short-all-in-does-not-reopen-betting rule. Bumps
  `action_legal_v`, so strict and loose checkpoints are different
  abstract games.
- **M-15 streaming checkpoint loader**: 1 MB chunks into destination
  atomics (~1 MB peak transient; was up to 4 GB).
- **M-31 arena refactor** of the vector subgame solver: flat
  `Vec<Node>` + `u32` child indices; last `unsafe` aliasing removed.
  Golden test pins `exploitability@50 = 0.658723` (river).
- **Phase 4.3**: `RuntimeSession::observe_action` updates the tracker
  with the SEARCH policy of kept-whole search roots.
- Duplicate-poker tournament / arena harnesses (CRN), `lbr_eval`,
  `expl_eval`, cross-process H2H example — the F9 measurement suite.
- Stack-depth parameter `PKR_STACK_BB` with `stack_bb_div2`
  fingerprinting; shadow state (`PKR_SHADOW=1`); `pkr-bot-pipe` live
  bot binary; `health` latency probe.
- Leduc CFR grid + NLHE eps sweep infrastructure (no shipping signal —
  seed variance dominates at 20M).

### Fixed
- ~50 review fixes (sessions 5–9): every Critical (C1–C10), every
  non-deferred Major, ~25 Minor from the 2026-10-10 code review.
  Includes the `pkr-bot-pipe` seat bug that gave seat 1 the wrong hole
  cards (invalidated one H2H run) and correlated tournament seeds
  (shifted-by-one).
- Perf triage P5–P18 closed: P1 (FMph verify-then-use), P10 (signature
  dispatch cache), P13 partial (1326-combo table), P17
  (`madvise(MADV_RANDOM)`) landed; P7 reverted (slower); P5/P8/P11/
  P15/P16 verified not actionable; P9 (UndoRecord slimming) deferred
  with a plan.

### Reverted
- **T2.2 river resolution** (2026-09-24 entry below): the 15 → 13 shift
  and 200 → 128 buckets were REVERTED after the eval comparison came
  back neutral. Code is back at `RIVER_TIER_SHIFT = 15`,
  `RIVER_BUCKETS = 200`. The shift is encoded in the
  `AbstractionFingerprint`, so pre-T2.2 and post-T2.2-revert artifacts
  are distinct and validated at load.
- Task 2.3a suit-isomorphism precompute port: tests caught
  over-identification of non-equivalent hands.
- Held-out BR split (audit item C1): BR policy does not generalize
  across deal sets when the hash is keyed on `cluster_id`.

### Runs
- `outputs/k7-baseline/` — SHIPPED (see K=7 above).
- `outputs/run1-combined/` — DO NOT SHIP: `eps=0.002 +
  PKR_STRICT_MIN_RAISE=1` at 100M regressed LBR by +261 mbb
  (−162.79 → +98.54) and in-sample BR at every eval point; reproduced
  byte-for-byte. Knobs bundled, cause not isolated; not worth a
  12-hour isolation run without a better reason.

## 2026-09-24 — post-audit landing (branch `audit-fixes`)

Landed the actionable subset of the F1–F9 audit plus the session's
algorithmic findings. Numbers below are on the **corrected** BR
estimator; every pre-`1b48865` `expl_mbb` is on the old (biased-low)
scale and is not comparable. (The later 2026-10-02 review further
established that even the corrected estimator is in-sample and
inflated; only seeded A/B deltas are meaningful — see
`docs/HISTORY.md`.)

### Correctness / safety
- **A2** Checkpoint magic v6 → v7; v6 files rejected with a clear
  error. `--fresh` flag; trainer refuses to silently start fresh over
  an existing checkpoint; `.prev` fallback.
- **A3** `dcfr::update_regret_i64` is overflow-safe (saturating add),
  uses exact u64 discount (no per-cell `i128` divide), no-op during
  warmup. `PKR_MOMENTUM` selects the update mode.
- **A4/A5/B8** CPU regression tests enabled. `snapshot()` reports
  regret-only stats.
- **B1** Pruned/illegal buckets no longer push NaN deltas.
- **B2** Dead FBRS pruning removed (regrets floored at 0).
- **B3** `FlushMode` struct; `PKR_F5_SEQUENTIAL`, `PKR_MOMENTUM` read
  once from env; non-finite deltas skipped in batched mode.
- **B4** Capacity accounting uses `allocated()`; Ctrl-C handler writes
  a final checkpoint; capacity stop writes a final checkpoint.
- **B5** Blueprint writer is atomic (`.tmp` + rename), fallible, and
  `quantize_cdf` always closes at 255 on the last non-zero action.
- **B6** mmap reader rejects versions > current; validates
  `cdf == keys*k`; `pod_read_unaligned` for the fingerprint.
- **B7** `GameState::legal_actions()` delegates to
  `legal_actions_into()` — single source of truth for the raise cap and
  all-in dedup, so fuzz tests validate the training path.
- **B9** GPU path gated behind `feature = "gpu"`.
- **B10** River abstraction fallback increments `FALLBACK_COUNTS[3]`.

### Metric (exploitability)
- **C2** Illegal buckets can no longer win the argmax; BR default for
  unknown infosets is the blueprint argmax, not Fold; BR iteration cap
  raised to 12 with fixed-point early exit.
- **C3** Promotion gate is at least 2 standard errors; eval seed fixed;
  `--eval-deals` default raised to 10000.

### Convergence (behind flags, default = old behaviour)
- **D1** `PKR_MOMENTUM=0` disables PCFR+ momentum-as-regret-increment.
- **D2** `PKR_AVG_POWER=p` weights strategy-sum averaging by `t^p`.

### Perf
- **E1** Exact u64 discount removes the per-cell `i128` divide.
- **E2** Regret matching drops the redundant `/SCALE`.
- **E3** `PKR_SKIP_FORCED=1` skips single-legal-action nodes.
- **E4** `valuenet::forward` uses stack arrays.

### Runs (old-estimator numbers, historical only)
- v21 momentum OFF, avg=0: 4443 · v22 avg=2: 3392 · v23: first
  C-patched metric run.

## 2026-09-24 — fast7 evaluator bug (regression found via A/B bisect)

Training quality regressed from ~5500 to ~9000 mbb at 5M iters after
the P1-a wiring; bisected to `Fast7Evaluator`. Three bugs, all fixed
with targeted regression tests (`fast7::fast7_bug_regressions`):

1. **Encoding mismatch**: fast7 used `rank * 4 + suit`; the rest of the
   crate uses `suit * 13 + rank`. Fixed to match.
2. **Flush path missed straight flush**: now enumerates all C(m,5)
   subsets of the suited cards.
3. **COMBOS_7_5 out-of-range for m=6**: combo indices filtered by
   `index < m`.

`find_rank_table()` now resolves against `CARGO_MANIFEST_DIR/../..` so
the parity tests actually run under nextest.

## 2026-09-24 — performance landing (post fast7 fix)

- **P1-f**: 480-byte `action_indices` table replaced by an 8-byte
  `bucket_of_action` array + `pick_in_bucket` (removes ~4 GB/s of
  memset).
- **UndoRecord**: `usize` fields shrunk to `u8`; size-assertion test.
- **add_sum_grouped**: non-CAS batch path for `apply_strategy_batch`.
- **Inline coverage**: `history_signature`, `terminal_payoff`,
  `legal_actions_into`, `action_bucket`, `flat_index_*`,
  `get_strategy_*`, `TableEvaluator::evaluate_hand`.
- All 266 workspace tests passed; ~48–61K it/s vs ~28–30K baseline.

## Earlier (initial development)

### Added
- `smoke.sh`: end-to-end proof — precompute, train, export, load.
- Checkpointing: `--checkpoint <path> --checkpoint-every <N>`, resume,
  `CompactRegretTable::with_capacity`, `Trainer::with_capacity`.
- `SolverHandle::debug_keys()`; `pkr-runtime` re-exports `SolverHandle`.
- `precompute` gained `hand_ranks` and `centroids` subcommands.

### Fixed
- `get_or_create_idx` no longer overflows `next_idx` past capacity.
- `write_blueprint` sorts keys defensively and normalizes the CDF via
  `get_average_strategy_into` (previously wrote raw strategy-sum and
  saturated to 255).
- `load_external_blueprint` borrow fix; `smoke.sh` absolute paths and
  `.smoke/` cleanup.
