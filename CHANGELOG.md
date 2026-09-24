# Changelog

All notable changes to pkr-sota will be documented here.

## Unreleased

### Added
- `smoke.sh`: end-to-end proof-of-concept — precompute, train 10 iters,
  export blueprint, load via pkr-runtime. Verified working.
- `load_external_blueprint` ignored test in `pkr-trainer` for verifying
  the file the real CLI produces is loadable.
- Checkpointing: `pkr-trainer --checkpoint <path> --checkpoint-every <N>`.
  Resumes automatically when the checkpoint file exists.
- `CompactRegretTable::with_capacity(n)` and `Trainer::with_capacity`.
- `SolverHandle::debug_keys()` for diagnostics and tests.

### Fixed
- `get_or_create_idx` no longer overflows `next_idx` past capacity at
  checkpoint time (CAS loop, clamp on save).
- `write_blueprint` sorts keys defensively and normalizes the CDF via
  `get_average_strategy_into` (previously wrote raw strategy-sum and
  saturated to 255).
- `pkr-runtime` re-exports `SolverHandle` at the crate root.
- `precompute` gained `hand_ranks` and `centroids` subcommands that
  `run.sh` already referenced.
- `load_external_blueprint` copies `FileHeader` instead of holding a
  borrow across `SolverHandle::new`.
- `smoke.sh` now uses absolute paths and cleans `.smoke/` before each
  run (fixes test-CWD mismatch and stale-checkpoint resumption).

### Changed
- Test wall time reduced ~8x via `with_capacity` in tests and a smaller
  valuenet training scope.
- `run.sh` and `justfile` updated with checkpoint flags and a clippy gate.
- `.gitignore` ignores `*.ckpt` and `.smoke/`.

## 2026-09-24 — post-audit landing (branch `audit-fixes`)

Landed the actionable subset of `docs/PKR_AUDIT_AND_FIX_PLAN.md` plus the
session's algorithmic findings. Numbers below are on the **corrected**
BR estimator; every pre-`1b48865` `expl_mbb` is on the old (biased-low)
scale and is not comparable.

### Correctness / safety
- **A2** Checkpoint magic v6 -> v7; v6 files are rejected with a clear
  error instead of mis-parsed. `--fresh` flag. Trainer refuses to
  silently start fresh over an existing checkpoint. `.prev` fallback.
- **A3** `dcfr::update_regret_i64` is overflow-safe (saturating add),
  uses exact u64 discount (no per-cell `i128` divide), and is a no-op
  identity during warmup. `PKR_MOMENTUM` selects the update mode.
- **A4/A5/B8** CPU regression tests enabled (no longer gated on the
  `gpu` feature). `snapshot()` reports regret-only stats.
- **B1** Pruned/illegal buckets no longer push NaN deltas.
- **B2** Dead FBRS pruning removed (regrets are floored at 0).
- **B3** `FlushMode` struct; `PKR_F5_SEQUENTIAL`, `PKR_MOMENTUM` read
  once from env; non-finite deltas skipped in batched mode.
- **B4** Capacity accounting uses `allocated()` (leaked slots counted);
  Ctrl-C handler writes a final checkpoint; capacity stop writes a
  final checkpoint.
- **B5** Blueprint writer is atomic (write `.tmp`, then rename),
  fallible (no `expect`), and `quantize_cdf` always closes at 255 on
  the last non-zero-probability action.
- **B6** mmap reader rejects versions > 4; validates `cdf == keys*k`;
  `pod_read_unaligned` for the fingerprint.
- **B7** `GameState::legal_actions()` delegates to
  `legal_actions_into()` — single source of truth for the raise cap and
  all-in dedup, so fuzz tests validate the training path.
- **B9** GPU path is gated behind `feature = "gpu"`; `pollster` is an
  optional dependency; `chunks(0)` panic is unreachable in the default
  build.
- **B10** River abstraction fallback now increments
  `FALLBACK_COUNTS[3]`.

### Metric (exploitability)
- **C2** Illegal buckets can no longer win the argmax; BR default
  action for unknown infosets is the blueprint argmax, not Fold; BR
  iteration cap raised to 12 with fixed-point early exit.
- **C3** Promotion gate is at least 2 standard errors; the eval seed is
  fixed so successive points are comparable; `--eval-deals` default
  raised to 10000.
- Held-out BR split (C1) was tried and reverted (`1b48865`): the
  infoset hash is keyed on `cluster_id`, so a BR policy fit on deal set
  A does not generalise to deal set B.

### Convergence (behind flags, default = old behaviour)
- **D1** `PKR_MOMENTUM=0` disables PCFR+ momentum-as-regret-increment
  (textbook CFR+: `r' = max(0, disc(r) + delta)`).
- **D2** `PKR_AVG_POWER=p` weights strategy-sum averaging by `t^p`
  (0 = uniform, 2 = DCFR gamma).
- **Production flags**: `PKR_MOMENTUM=0 PKR_AVG_POWER=2`.

### Perf
- **E1** Exact u64 discount in A3 removes the per-cell `i128` divide.
- **E2** Regret matching drops the redundant `/SCALE`; single helper
  `regret_match_into`.
- **E3** `PKR_SKIP_FORCED=1` (default off) skips single-legal-action
  forced-move nodes.
- **E4** `valuenet::forward` uses stack arrays for the two hidden
  activations.
- `apply_strategy_batch` chunk sizing matches `flush_cpu_batch`.

### Runs
- v21 momentum OFF, avg=0: 4443 (old estimator)
- v22 momentum OFF, avg=2: 3392 (old estimator), flat within 2% over 10M
- v23 = v22 flags, 200M iters, first C-patched metric
  (`PKR_MOMENTUM=0 PKR_AVG_POWER=2`)

## 2026-09-24 — fast7 evaluator bug (regression found via A/B bisect)

After landing all perf commits (P1-a through P3-b), training quality
regressed from ~5500 to ~9000 mbb at 5M iters. Bisected to P1-a
(TableEvaluator -> Fast7Evaluator wiring).

Root cause: fast7's card encoding was `rank * 4 + suit` but the rest of
the crate (slow.rs, hand_ranks.bin, precompute) uses `suit * 13 + rank`.
The parity tests were silently skipping because `find_rank_table()`
returned None when CWD didn't match the workspace root.

Three concrete bugs, all fixed:

1. **Encoding mismatch.** `evaluate_hand` decoded `c & 3` as suit and
   `c >> 2` as rank. Fixed to `c / 13` and `c % 13`.
2. **Flush path missed straight flush.** Top-5-by-id spades of
   A,T,5,4,3,2 gives ace-high flush but the actual best 5 is
   A,5,4,3,2 (wheel straight flush). Now enumerates all C(m,5)
   subsets of the suited cards.
3. **COMBOS_7_5 out-of-range for m=6.** The 7-slot combo table's
   first 6 entries referenced index 6 when only 6 suited cards
   were present. Now filtered by index < m.

Test fixes:
- `find_rank_table()` resolves against `env!("CARGO_MANIFEST_DIR")/../..`
  so the parity tests actually run under nextest.
- Targeted regression tests for each bug class:
  `fast7::fast7_bug_regressions::*`.

After the fix, P1-a was re-applied (commit 48a20a5). Expected to match
the pre-perf baseline on training quality with the ~15-20% it/s win.


## 2026-09-24 — Performance landing (post fast7 fix)

After the fast7 bugs were fixed and P1-a re-applied, the following
additional perf improvements were landed on top of the session's
earlier audit fixes:

### Data-layout improvements
- **P1-f**: replaced the 480-byte `[[usize; 10]; K]` `action_indices`
  table in `traverse` with an 8-byte `bucket_of_action` array and a
  `pick_in_bucket(bucket, ordinal)` helper. Removes ~4 GB/s of memset
  per process at ~8M node visits/sec.
- **UndoRecord**: shrunk `history_len`, `board_len`, `actor` from
  usize (8B) to u8 (1B each). Size assertion test guards against
  silent growth.

### Compute improvements
- **add_sum_grouped**: non-CAS batch path for `apply_strategy_batch`.
  After sort-dedup each (idx, action) is unique per parallel group, so
  the CAS loop's branch+fence are unnecessary.

### Inline coverage
- `#[inline]` on `history_signature`, `terminal_payoff`,
  `legal_actions_into`, `action_bucket`, `flat_index_*` helpers,
  `get_strategy_and_idx`, `get_strategy_into`,
  `TableEvaluator::evaluate_hand`.
- `#[inline(always)]` already present on `load_rm`/`store_rm`,
  `off_rm`/`off_sum`, `regret_match_into`, cache helpers.

### Verification
- All 266 workspace tests pass.
- 30-second bench on the M1: ~48-61K it/s (vs ~28-30K it/s baseline).
- 20M training run reproduces the same expl_mbb trajectory as the
  pre-perf baseline:
  - v23a3   @ 20M: 5908  (old code)
  - v25fast7 @ 20M: 5767  (P1-a + fixed fast7)
  - v25final @ 20M: 5735  (all of the above)
  - all within 1σ.
