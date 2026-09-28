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

END OF HANDOFF
