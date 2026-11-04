# pkr-sota: Current Status

**Last updated:** 2026-11-03 (docs synthesis; supersedes the old
`status.md`, `INDEX.md`, and the session-9 handoff).

This is the source of truth for what is actually implemented and what the
numbers say. The full experiment history (including dead ends) lives in
[`HISTORY.md`](HISTORY.md); architecture lives in
[`ARCHITECTURE.md`](ARCHITECTURE.md).

## Where the bot stands

**Tier: weak-bot.** The shipped configuration is the **K=7 baseline**
(100M iterations, seed 42, 4 threads, cap 60M, finished in 19,962 s at
5,009.6 it/s) — decision to ship: 2026-10-10.

| metric | K=7 baseline | run 1 (combined) | note |
|---|---:|---:|---|
| LBR (20k deals, seed 42) | **−163 ± 104 mbb** | +99 ± 114 mbb | lower bound, one greedy exploiter |
| H2H search-vs-blueprint, seed 42 | +0.958 chips (t=+5.28) | +1.105 (t=+6.22) | both clear the 2·SE gate |
| H2H search-vs-blueprint, seed 43 | +1.279 chips (t=+7.02) | +1.078 (t=+5.94) | both clear the 2·SE gate |
| in-sample BR @100M | 7,922 | 8,062 | NOT a strength measure |

Run 1 (`eps=0.002 + PKR_STRICT_MIN_RAISE=1` at 100M) was a **DO NOT
SHIP** verdict: LBR regressed by +261 mbb, reproducible byte-for-byte
with the current binary, and in-sample BR is worse at every eval point.
The two knobs were bundled so the cause is not isolated; neither is
compelling enough for another 100M run without a better reason. **The
K=7 baseline remains the shipped checkpoint.**

Historical K=6 reference points (different abstract game, not
comparable to K=7): v4 LBR ≈ 821 mbb, v5 LBR ≈ 183 mbb (samples=2000);
cross-process H2H showed V4 and V5 are *equal* in self-play while V5 is
genuinely harder to exploit. See HISTORY.md.

**Honesty rules that apply to every number in this repo:**

1. **Not superhuman.** The project's own reviews place it far from
   competitive HUNL solvers; the realistic near-term goal is a strategy
   that is *provably not trivially exploitable*.
2. **Absolute exploitability numbers are inflated.** The in-sample BR
   estimator behind historical numbers is known-biased (it fell
   3796 → 1707 → 1222 as eval deals went 5k → 20k → 40k). Only seeded,
   relative A/B comparisons between runs are meaningful.
3. **LBR is one greedy exploiter and has misled three times.** Always
   cross-check with the cross-process H2H harness (CRN, fixed bot).
4. **K=6 history is closed.** K=7 shipped 2026-10-10; earlier
   checkpoints are unloadable by design and their numbers must not be
   compared with K=7 numbers.

## Search (runtime)

- **Vector river search HELPS.** H2H vs blueprint on V5, 4 independent
  seeds: +1.18 / +0.82 / +0.97 / +1.16 chips/deal (all t > 4). On V4:
  LBR 821 → 444, H2H +1.25.
- **Shipped safe config = the heuristic BOUND** (`--search-bound`; live
  gate `PKR_VSEARCH=1 PKR_VSEARCH_BOUND=1`). Bound safety PROVEN
  (`final_br ≤ blueprint_br`). Deadline enforced inside BR walks.
- **CFR-D gadget: PROVEN safe** in sampled form (bp_br 9.56 vs
  gadget_br 0.63) and vector form (2026-10-09; EV-based terminate value;
  worst delta −0.94 over 4 subgames).
- **Turn search is offline-only** (~45 s per decision at both 10 and 50
  inner iterations). Depth-limited **flop search is measured
  infeasible** at the current abstraction resolution without a
  coarsened leaf (47× the turn tree; see HISTORY.md).

## What works

- **Training pipeline.** Precompute → train → export → load → query,
  verified by `./smoke.sh` and the golden-hash CI gate. Deterministic:
  same seed + inputs → byte-identical blueprint at any thread count.
- **Runtime lookup.** `SolverHandle::get_advice_fast` p50 ≈ 42 ns,
  p99 ≈ 84 ns on a large blueprint.
- **River-only subgame solving** via `RuntimeSession` in the shipped
  bound config.
- **Live bot binary.** `pkr-bot-pipe` (JSON line in, one action token
  out) shares the code path used by tournament / arena / LBR.
- **Shadow state.** `shadow.rs` keeps an in-tree view advanced by
  translated actions; blueprint hashing and the range tracker use it.
- **Stack depth param.** `PKR_STACK_BB` (default 100), fingerprinted via
  `stack_bb_div2` — a 100bb checkpoint refuses to load into a 200bb run.
- **Strict min-raise (opt-in).** `PKR_STRICT_MIN_RAISE=1` enables the
  correct short-all-in-does-not-reopen rule; default OFF (the shipped
  checkpoint was trained on the loose game). Different `action_legal_v`
  → different abstract game; no cross-loading.
- **Slumbot client (written, UNVERIFIED).** `scripts/slumbot_client.py`
  wire format has never been validated against `sample_api.py`; no live
  run yet.

## Audit status (closed 2026-10-12)

The F1–F9 external audit (2026-09-30) and the follow-up code review
(2026-10-10; 3 High / 6 Medium / 7 Low / 15 smells / 6 doc bugs) are
closed: every Critical (C1–C10), every non-deferred Major and ~25 Minors
landed across sessions 5–9 (~50 fixes). The bug class "K=6 leftovers
breaking K=7" is structurally closed by `ci/scripts/check-no-hardcoded-k.sh`
plus the shared `first_legal_by_bucket` / `sample_legal_index` helpers.
A November 2026 re-audit of the whole workspace (five subsystem
reports, ~50 confirmed findings, every fix TDD'd with a regression
test) is CLOSED: the codebase is at 480 passing tests, clippy-clean
under `-D warnings`, rustfmt-clean, and doctest-clean. All fixes and
the deliberate not-fixed list (golden-hash-locked items) are recorded
in CHANGELOG.md.

## What isn't there

- **No external-strength evaluation.** Nothing has played Slumbot or any
  public API. The client exists; wire format unverified. This is the
  single highest-value next step.
- **No depth-limited flop search.** Measured infeasible without a
  coarsened leaf (512-board ~12 h precompute, or 2048-board ~246 h);
  needs its own strength-for-speed A/B before starting.
- **No suit isomorphism** for flop/turn rebuilds (attempt reverted —
  tests caught over-identification).
- **P9 UndoRecord slimming deferred.** Shrinking `UndoRecord` (75% of
  the 3,592-byte `GameState`) touches the on-disk checkpoint format and
  needs a versioning plan.
- **No 200bb build launched** (the stack param + fingerprint exist).

## Background jobs

NONE. All runs are complete: `outputs/k7-baseline/` (SHIPPED) and
`outputs/run1-combined/` (DO NOT SHIP). There are no locks in `/tmp` and
no queued experiments.
