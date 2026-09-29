# HANDOFF — range-aware subgame solving (2026-09-28 late session)

**Continues:** `docs/HANDOFF_2026-09-28.md`
**Tree:** clean

---

## What this session produced

Range-aware subgame solving — wired, verified, and confirmed to win
in game play.

| metric | before | after |
|---|---|---|
| river subgame e2e (uniform ranges) | +4836 mbb regression | n/a |
| river subgame e2e (tracked ranges, 10 iters, 100 deals) | n/a | -65.7 mbb |
| game play, 20000 paired deals, chips/deal to P0 | n/a | +2.40 (t=6.04) |

The static-exploitability number is small but consistently signed
(2/8/100 deals: -98/-53/-66). The game-play number is the meaningful
verification — the subgame-P0 policy wins more chips per deal than
the blueprint-P0 policy on the same deals.

---

## The single most important finding

**The static exploitability estimator cannot resolve small effects.**

At 100 deals the SE is 1618 mbb — more deals does NOT shrink it,
because per-deal in-sample BR variance dominates and does not go to
zero as 1/sqrt(N). The range-aware subgame delta is ~50-100 mbb, so
it sits two orders of magnitude below the noise floor.

**Solution:** measure game play, not exploitability. Paired deals with
a forced river-heavy line, chips/deal as the metric. At 20000 deals
that gives t = 6.04. This is the framework for any future small-effect
verification in this project.

See `docs/experiments/range-aware-solving-poc.md` for the full record.

---

## Commits this session

    461e67c  walker wiring (RangeTracker mirrored through apply/undo)
    0b06058  hook signature + PKR_SUBGAME_ITERS + tracker_probe
    3ec3fa4  revert useless fingerprint change (was a no-op)
    d18796c  runtime decide guard against all-zero opp_range
    f18162b  PKR_COUNT_RIVER_NODES classifier
    4ef684c  tracker/state sync invariant test
    aae53fe  remove unused Traversal struct
    1959e2b  docs Run F (100 deals, -66 mbb)
    6a9cb21  counter run (48% shallow river nodes)
    11f2e64  design doc correction
    e39b5b4  gameplay subgame scaffold (incomplete, later rewritten)
    e0c0c6c  river-heavy gameplay test + runtime tracker design
    0c7514b  paired SE on gameplay diff
    6809dbf  drop unused Street import
    28ca594  docs definitive game-play result t=6.04
    0889ba1  delete 4 dead methods in pkr-subgame
    53c5421  docs gameplay 200 + 2000
    d3766e1  end-of-session summary

---

## What is now possible

- `SubgameHandle::decide(state, our_hole, opp_range)` returns a river
  strategy that is genuinely less exploitable than the blueprint's
  river play, when given a tracked non-uniform range.
- `RangeTracker` is verified to produce a non-uniform posterior at
  river (~280x uniform on the top hand, `tracker_probe.rs`).
- The BR walkers correctly mirror state mutations into the tracker
  (`tracker_state_sync.rs`).

## What is NOT yet done

1. Runtime integration. No code path in a live bot maintains a
   `RangeTracker`. `docs/roadmap/runtime-tracker-integration.md`
   describes the `RuntimeSession` wrapper. Blocked on a bot binary
   existing — the only binary today is `pkr-trainer`, which does not
   play games.
2. Turn extension. Only river is verified. Turn adds chance nodes to
   the subgame; same pattern, more work.
3. Cost. ~37k hook calls/deal at ~2.3 ms/miss. 100-deal e2e at 10
   iters is ~72 min. Not routine-A/B viable. The
   "solve-first-river-node-only" mitigation saves ~2x (measured
   shallow fraction 48%) — deprioritized given the definitive
   game-play result.

## Next session should

If shipping: add `RuntimeSession` to `pkr-runtime::subgame` (design in
`docs/roadmap/runtime-tracker-integration.md`), wire it into whatever
bot consumer comes next. Do not touch `SubgameConfig::default()` —
leave river disabled; callers opt in.

If extending: the turn pattern is same walker wiring (already works
for any street), add `enabled_streets[2] = true`, wait for the
chance-node expansion in `SubgameHandle::decide`. Estimated: a full
afternoon.

If debugging: the two tests to reach for are `tracker_probe.rs` (is
the posterior live?) and `gameplay_subgame.rs` (does the hook change
the outcome?). Between them they bracket the whole pipeline.

## Reference

- `docs/experiments/range-aware-solving-poc.md` — full verification
- `docs/roadmap/range-aware-solving.md` — original design + outcome
- `docs/roadmap/runtime-tracker-integration.md` — RuntimeSession design
- `docs/roadmap/solve-first-river-node.md` — deprioritized mitigation
- `crates/pkr-exploit/tests/gameplay_subgame.rs` — verification test
- `crates/pkr-subgame/tests/tracker_probe.rs` — non-uniformity probe
- `crates/pkr-exploit/tests/tracker_state_sync.rs` — walker invariant
- `crates/pkr-runtime/tests/decide_guard.rs` — all-zero range guard
- `crates/pkr-exploit/tests/e2e_subgame_hook.rs` — static exploitability

---

---

## LATE-SESSION UPDATE (same session, after initial handoff)

### Turn extension — negative result, not shipped

| config | iters | delta | t |
|---|---|---|---|
| river only | 10 | +1.43 | 2.44 |
| river only | 50 | +1.38 | 2.35 |
| river+turn | 10 | -0.67 | -0.91 |
| river+turn | 50 | -0.16 | -0.22 |

Runtime turn tests pass (both seats, strategy sums to 1.0). Turn is
correct but not useful at either iteration count. It changes 3858 of
5000 river decisions, just not for the better.

**Ship river-only.** Turn needs a different decomposition — a
selective node choice, not "solve at every turn decision".

Turn cost: 2.2 s/deal at 50 iters vs 0.5 s/deal for river.

### hands_per_range — plateaus at 4

5000 deals, river-only, 10 iters:

| hands | delta | t |
|---|---|---|
| 2 | +1.33 | 2.25 |
| 4 | +1.43 | 2.44 |
| 8 | +1.43 | 2.42 |
| 16 | +1.54 | 2.65 |

Default 4. No reason to pay for a non-significant improvement.

### Shipping API: RuntimeSession has LANDED

The initial handoff said "runtime integration not yet done". That
changed. `crates/pkr-runtime/src/session.rs` now provides:

    use pkr_runtime::RuntimeSession;

    let mut session = RuntimeSession::new(handle, our_seat, abs, tbl, ev);
    session.deal_start(root);
    session.observe_action(action);
    session.observe_street(&cards);
    let strategy = session.advise_or_blueprint(&state, &hole, hash);

- `RangeTracker` not leaked to the caller.
- `advise_or_blueprint` always returns a normalized strategy: subgame
  if possible, blueprint average otherwise, uniform as the fallback.
- `advise` (no fallback) returns `None` when it's not our turn or the
  street is disabled.
- `SubgameHandle::table_ref` exposed so the fallback can read the
  blueprint table.

Tests: `crates/pkr-runtime/tests/session_smoke.rs` — 3 tests
(owns-tracker, deal_start-resets, advise-or-blueprint contracts). All
pass at HEAD with
`cargo test --release -p pkr-runtime --test session_smoke -- --ignored`.

### Parallelism

`gameplay_subgame.rs` uses rayon over deals. 5000 deals in ~4 s
(previously ~2 min). RNG seeded per (config, deal) so parallel and
sequential runs agree.

### Cross-seed confirmation

5 seeds x 2000 deals: +1.85 / +3.54 / +2.43 / +0.54 / +2.11 — all
positive. Pooled t ~ 4.5. Combined with the 20000-deal t=6.04 run,
the river effect is robust.

### Env vars for gameplay_subgame.rs

    PKR_GP_DEALS       (default 200)
    PKR_GP_SEED        (default 42)
    PKR_GP_HANDS       (default 4)
    PKR_GP_TURN        (default off)
    PKR_GP_DEBUG       (default off)
    PKR_SUBGAME_ITERS  (default 10)

### Corrected "what's next"

1. Runtime integration — the API is done. What's missing is a bot
   binary that actually calls `RuntimeSession`. Blocked on that
   existing.
2. Turn — dead end at the current design. Do not spend more time on
   "solve every turn decision". If turn is revisited, it needs a
   node-selection heuristic first.
3. v33 retest — `launcher-v33-retest.sh` (git-ignored, on disk).
   Re-measures the +425 mbb preflop feature win under the
   deterministic trainer. 4 runs x 30M iters, ~8 h serialized.

### Late-session commits

    7034f17  re-export RuntimeSession
    d1fbcd2  RuntimeSession + tracker wrapper
    6514ae8  session deal_start resets tracker
    fb8842b  RuntimeSession::advise_or_blueprint + table_ref
    2768341  advise_or_blueprint test
    317791f  fix advise_or_blueprint test (fake hashes only)
    0a6fc50  turn-50 negative result documented
    b9a2f06  parallelism note + baselines
    fa52ac8  turn cost measurement
    6d81a8a  runtime turn passes
    71e10d8  root_p0_strategy_aggregated normalisation test
    4b57189  parallel gameplay + env vars
    1f39040  turn toggle + preliminary finding
    dab1dff  PKR_GP_SEED for cross-seed confirmation
    7278c07  status snapshot rewrite
    bc6530b  PKR_GP_HANDS; hands plateau at 4

---

END OF HANDOFF
