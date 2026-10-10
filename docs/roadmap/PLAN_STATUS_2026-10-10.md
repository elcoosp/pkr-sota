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
