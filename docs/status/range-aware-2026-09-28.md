# Range-aware subgame solving — status 2026-09-28 (final)

## Result

**Game play: subgame-P0 beats blueprint-P0 by +2.40 chips/deal
(t=6.04) on 20000 paired river-heavy deals, seed 42.**

Reproduced across 5 seeds at 2000 deals each: all positive.

Static exploitability: -65.7 mbb at 100 deals, sign consistent across
2/8/100 deals. Small magnitude, but the game-play number is the
meaningful verification.

## What works

- `RangeTracker` threaded through the BR walkers, reach
  `SubgameHook::strategy` as `opp_range`. `tracker_probe.rs` proves
  posterior max mass ~280x uniform at a river decision.
- `tracker_state_sync.rs` proves the walkers' tracker mirroring is
  invariant under apply/undo.
- `SubgameHandle::decide` returns a strategy that beats the blueprint
  at river when given a tracked non-uniform range.
- **`RuntimeSession`** (`crates/pkr-runtime/src/session.rs`) wraps
  `SubgameHandle` + `RangeTracker`. Bot callers call `deal_start`,
  `observe_action`, `observe_street`, `advise`; the tracker is not
  leaked. This is the shipping API.
- `session_smoke.rs` (ignored, needs ckpt) validates the wrapper
  contract.

## What doesn't work

- **Turn at 10 inner CFR iterations.** river+turn 5000-deal gameplay:
  -0.67 chips/deal (t=-0.91). Correctness is fine (runtime turn test
  passes both seats); the solve quality is insufficient. Retest at 50
  iters in flight.
- **Cost.** ~37k hook calls/deal at ~2.3 ms/miss. 100-deal e2e at 10
  iters ~72 min. River-50 = 0.5 s/deal, turn-50 = 2.2 s/deal.
- **`SubgameConfig::default()`** has all streets disabled; callers must
  opt in. That is intentional — without a tracker the uniform-range
  regression returns.

## Where to look

| path | what |
|---|---|
| `docs/experiments/range-aware-solving-poc.md` | Full record |
| `docs/roadmap/runtime-tracker-integration.md` | RuntimeSession design |
| `docs/roadmap/range-aware-solving.md` | Original design + outcome |
| `docs/roadmap/solve-first-river-node.md` | Cost mitigation (deprioritized) |
| `crates/pkr-runtime/src/session.rs` | Shipping API |
| `crates/pkr-exploit/tests/gameplay_subgame.rs` | Verification test |

## Commits

- `461e67c` walker wiring
- `0b06058` hook signature + `PKR_SUBGAME_ITERS` + `tracker_probe`
- `3ec3fa4` revert useless fingerprint change
- `d18796c` runtime decide guard + `decide_guard`
- `f18162b` `PKR_COUNT_RIVER_NODES` classifier
- `4ef684c` `tracker_state_sync`
- `aae53fe` remove unused `Traversal`
- `1959e2b` docs Run F
- `6a9cb21` counter run (48% shallow)
- `11f2e64` design doc correction
- `e39b5b4` gameplay scaffold
- `e0c0c6c` river-heavy gameplay test
- `0c7514b` paired SE
- `28ca594` definitive game-play t=6.04
- `0889ba1` delete dead methods in pkr-subgame
- `4b57189` parallel gameplay + `PKR_SUBGAME_ITERS`
- `1f39040` turn toggle + preliminary finding
- `fa52ac8` turn cost
- `6d81a8a` runtime turn passes
- `d1fbcd2` **`RuntimeSession`** shipping wrapper
