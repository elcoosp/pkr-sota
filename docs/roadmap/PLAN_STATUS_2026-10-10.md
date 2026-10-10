# Plan status snapshot - 2026-10-10 (mid-session, k7base running)

Recon at HEAD afb8b46 (+ the 7-bucket docstring commit this script).
k7base PID 59819 still training; k7h2h queued behind it.

## Landed since PLAN_STATUS_2026-10-09.md

- 0.1 stack param `PKR_STACK_BB` + fingerprint field `stack_bb_div2`
  (`crates/pkr-core/src/state.rs`, `crates/pkr-core/src/abstraction.rs`).
  Zero literal `GameState::new(200.0)` in crates/binaries.
- 0.2 duplicate-deal H2H: `self_tournament_is_zero` test in
  `crates/pkr-fuzz/src/tournament.rs`. **`--ckpt-dir` still absent.**
- 0.3 LBR JSON line: `crates/pkr-exploit/src/bin/lbr_eval.rs:187,190`.
- 0.4 Slumbot client: `scripts/slumbot_client.py` (46 lines).
- 1.1 shadow state: `crates/pkr-runtime/src/shadow.rs` (196 lines),
  wired via `session.rs` under `PKR_SHADOW`.
- 1.2 translation log: JSONL `/tmp/translate.jsonl` in
  `translate_live.rs:127`.
- 1.3 bot binary: `crates/pkr-runtime/src/bin/pkr-bot-pipe.rs` (221 lines).
- 2.1 preflop-exact default ON (`preflop_exact_enabled`, `lib.rs:55`).
- 2.2 V5 signature opt-in (`PKR_SIG_V5=1`) with callers in `expl_eval.rs`,
  `lbr_eval.rs`.
- 2.3 histogram module (`histogram.rs`, 375 lines; river_equity,
  turn_hist, flop_hist, cdf, emd_l1). **`canonicalize` (2.3a) missing.**
- 2.4 K=7 action abstraction landed (NUM_ACTION_BUCKETS=7,
  BET_SIZINGS=[0.5,1,2,3], JAM_BUCKET=6).
- 3.1 pruning wired (`PKR_PRUNE`, `PKR_PRUNE_WARMUP=20_000_000`); metrics
  column `pruned_actions`.
- 3.4 `PKR_PHASE_PROFILE`.
- 4.1 vector solver ported (`crates/pkr-subgame/src/vector.rs`, 1725 lines)
  with `exploitability`, `br`, `root_strategy_all`.
- 4.2 acceptance tests present in vector.rs.
- 4.4 CFR-D gadget (`Node::Const`, `gadget_player`, both sampled and
  vector variants).
- 4.6 Show cache (`Arc<ShowTable>` + `show_cache: HashMap<u64, Arc<..>>`).
- Review-fix batch 1a-1d applied; CI gate now covers code literals and
  stale "6-bucket" doc phrases.

## Still open

- **0.2 `--ckpt-dir`** (trainer periodic checkpoints). Real gap.
- **2.3a suit `canonicalize`** (abstraction precompute). Real gap.
- **3.2 `--iters-per-sync` CLI surface.** Field exists in
  `binaries/pkr-trainer/src/main.rs:140`, but sweep not run and default
  not documented in the plan.
- **3.3 wall-clock training cap (`BENCH_SECONDS`).** No such flag exists.
  `time_budget_ms` only covers search subgames.
- **4.3 wire `set_range` after search decisions.** `RangeTracker::set_range`
  exists (`range_tracker.rs:297`) but has NO callsite; the search path
  does not update our own range after playing the search strategy.
- **4.5 turn budget.** `session.rs:259` note: turn decision ~52 s because
  the bound's BR walks are outside `time_budget_ms`.
- **Phase 5 depth-limited flop.** `crates/pkr-cfr/src/riversolve.rs` is
  river-only (`state.street == Street::River` at :268, :326). Not started.
- **Phase 6 200 bb build + Slumbot run.** Client + stack param exist; no
  200 bb checkpoint yet; no live Slumbot number.
- **Leduc 8-config grid.** Only baseline finished; harness binary at
  `target/release/pkr-leduc-check`.

## Decision rule reminder (plan §3)

Ship only if `Δ_LBR <= -2*SE_combined` OR `Δ_duplicate-H2H >= +2*SE`,
with >=2 seeds when |Δ| < 300 mbb.

## Corrections after deeper recon (2026-10-10, k7base still running)

Three items earlier marked "still open" are already implemented under
different names:

- Task 0.2 (checkpoint-dir): implemented as `--ckpt-per-eval`
  (`binaries/pkr-trainer/src/main.rs:241`); writes
  `<checkpoint-stem>.<iter>.ckpt` at each eval point. Functionally the
  same deliverable the plan asked for.
- Task 3.2 (iters-per-sync): `--iters-per-sync` exists at
  `main.rs:139`, default 2048. Its docstring records a v36 sweep
  (512 vs 2048: 2048 gives ~20% more throughput at 5M iters with no
  measurable convergence cost). Pre-dates K=7; a re-run is optional.
- Task 3.3 (wall-clock training cap): `--bench-seconds` at
  `main.rs:114`, default 0 (off).

Revised plan status:

- Phase 0: 5/5 done.
- Phase 3: 3.1 (pruning wired), 3.2 (flag + sweep done), 3.3 (flag),
  3.4 (profile flag) done.
- Phase 4.3 (our-range update after a search decision): `RangeTracker::
  set_range` exists (`range_tracker.rs:297`) but has no callsite. The
  `observe_action` path updates our own range using the BLUEPRINT
  probability, not the SEARCH probability. Live default is RIVER-ONLY
  so this is MOOT for the shipping path; it becomes a bug only if
  turn or flop search is enabled.
- Phase 4.5 (turn budget): `session.rs:259` note — a turn decision
  costs ~52 s because the bound's BR walks fall outside
  `time_budget_ms`.
- Phase 5 depth-limited flop: `crates/pkr-cfr/src/riversolve.rs` is
  river-only (`state.street == Street::River` at :268, :326).
- Phase 6 200 bb + Slumbot: client + `PKR_STACK_BB` exist; no 200 bb
  checkpoint yet; no live Slumbot number.
- Leduc 8-config grid: only baseline finished; harness at
  `target/release/pkr-leduc-check`.

### Note on Task 2.3a (suit canonicalisation)

The session-7 handoff recorded "2.3(a) suit-iso measured 1.21x ->
declined." That measurement covered RUNTIME lookup cost, which is
dominated by table misses and cannot approach the theoretical 24x.
The plan's ~20x claim is about PRECOMPUTE time: the (hole, board)
space collapses by 24x. The two are different measurements. A
precompute-only canonicaliser is added alongside this note in
`crates/pkr-abstraction/src/histogram.rs::canonicalize`, with tests.
It is NOT wired into the precompute path yet; measure its precompute
time on a 100k-entry sample before adopting, per plan §3.

## Task 2.3a attempt REVERTED (2026-10-10)

A suit canonicaliser was added to `histogram.rs` and tested. The
permutation-invariance property held for 200 random (hole, board) pairs
under all 24 suit permutations, but exhaustive enumeration of the flop
space over-identified orbits:

  raw 5-card sets enumerated: 2,598,960
  distinct canonical outputs:   543,101
  expected (see below):       1,082,900

Expected count: the correct S_4 action is on the (hole, board) pair
space, not on 5-card sets. |X| = C(52,2) * C(50,3) = 25,989,600.
The action is free (no nonzero-fix: a 5-card set with 2-hole/3-board
split cannot be invariant under any nontrivial suit permutation
because 5 is not divisible by 4), so |X/S_4| = 25,989,600 / 24 =
1,082,900. Observed 543,101 is ~half that: the canonicaliser maps
different orbits to the same output.

Root cause: the sort key `(hole_rank_mask, board_rank_mask)` per suit,
with stable sort on ties, uses the ORIGINAL suit index to break ties.
Two suits with identical (hole_mask, board_mask) are assigned their
canonical positions based on which original suit label was smaller,
which is NOT orbit-invariant. Two (hole, board) pairs in different
orbits but with the same per-suit rank-mask multiset, differing only
in how the two tied suits were cross-assigned, collapse to the same
canonical output.

Reverted: the function and its tests are removed. The session-7 note
("2.3(a) suit-iso measured 1.21x -> declined") still stands; this
attempt does not change that verdict. A correct quotienting would
either (a) key on per-rank cross-suit structure across the whole
5-card set (which suits share which rank), or (b) take the
lexicographically minimal image over all 24 permutations (trivially
correct, O(24) per input, still gives the ~24x precompute-space
reduction). Worth doing only if the Phase 2.3 turn/flop precompute
budget proves to need that reduction -- unproven.
