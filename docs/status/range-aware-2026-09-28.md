# Range-aware subgame solving — status at 2026-09-28 ~17:50

## What works

- `RangeTracker` is threaded through the BR walkers
  (`collect_cfv`, `walk_fixed`) and reaches `SubgameHook::strategy` as
  `opp_range`. Verified end-to-end via `tracker_probe.rs` (posterior
  max mass ~280x uniform at a river decision) and via hook-debug traces
  during the 1-deal instrumentation run.
- 8-deal e2e at 10 inner CFR iterations: delta **-53.2 mbb**.
  Consistent with 2-deal (delta -98.2).
- 100-deal e2e at 10 iters running (started 17:31).

## What doesn't work

- `PKR_SUBGAME_ITERS=1` gives essentially uniform river strategy, which
  makes the e2e delta positive (+708). If you want a fast sanity check
  use `>= 10`.

## What's unresolved

- **Magnitude.** At 8 deals, SE ~434; -53 is below one sigma.
  The 100-deal run should reduce SE to ~120, still not conclusive at
  -53, but the sign is now consistent across three configurations.
- **Cost.** 8-deal / 10-iter is 346s; 100-deal ~70 min. Acceptable
  but not for routine A/B. Mitigation "solve-first-river-node-only"
  design exists in `docs/roadmap/solve-first-river-node.md`, with
  counter instrumentation landed (commit `f18162b`) and awaiting
  a measurement run.
- **Runtime integration of the tracker.** `SubgameHandle::decide`
  already accepts `opp_range`. The runtime does not yet maintain a
  tracker. That's the shipping gap.

## Commits this session

- 461e67c — walker wiring
- 0b06058 — hook signature + PKR_SUBGAME_ITERS + tracker_probe
- 3feceb0 / 46c1b0d / e3ff4b3 — docs run A-D
- 3ec3fa4 — revert useless fingerprint change
- d18796c — runtime decide guard + decide_guard tests
- f18162b — PKR_COUNT_RIVER_NODES classifier
- 4ef684c — tracker/state sync invariant test
- aae53fe — remove unused Traversal struct
