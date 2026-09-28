# Range-aware subgame solving — PoC result (2026-09-28)

**Status:** ⚠️ WIRING VERIFIED, OUTCOME UNRESOLVED.

The RangeTracker is threaded correctly into the hook (probe + 1-deal
hook-debug confirm non-uniform posteriors arrive at the solve). The
e2e outcome, however, is dominated by in-sample BR variance and by the
inner CFR iteration budget. Do not read the 1-deal number as evidence
of an improvement; read this doc in full.

## What changed

The `SubgameHook::strategy` signature gained a fourth parameter:

    opp_range: &[f64; N_HANDS]

`crates/pkr-exploit/src/best_response.rs` walkers (`collect_cfv`,
`walk_fixed`) now build a `RangeTracker` per deal and mirror every
`state.apply_action_in_place` / `state.undo_action` /
`state.advance_street_in_place` into it. When the hook fires at an
agent decision node, the walker passes `tracker.range(1 - actor)` as
`opp_range`. `SubgameHandle::decide` builds its P1 range from a weighted
sample of that posterior rather than a uniform prior.

Commits:
- `461e67c` — walker wiring (`RangeTracker` mirrored through apply/undo)
- `0b06058` — hook signature threading + `PKR_SUBGAME_ITERS` + `tracker_probe.rs`

## The measurements (both runs below)

Both runs use seed 42, v34long checkpoint, `PKR_E2E_HANDS=4`,
`PKR_BR_ITERATIONS=1`, `PKR_SUBGAME_ITERS=1`.

### Run A: 1 deal

| config | expl_mbb | delta |
|---|---|---|
| blueprint only | 8981.0 | — |
| tracked-range subgame | 4513.2 | **-4467.8** |

**This number is not evidence of an improvement.** At 1 deal the
in-sample BR overfits the single deal completely: the blueprint alone
reads 8981 mbb, versus 2414 at 8 deals. Any strategy that deviates
from the blueprint at the river looks "less exploitable" at 1 deal
because the clairvoyant BR has nothing to overfit against. The
+4836 uniform-range regression number has the same problem in reverse.

### Run B: 8 deals

| config | expl_mbb | delta |
|---|---|---|
| blueprint only | 2414.6 (SE 434.5) | — |
| tracked-range subgame | 3123.0 (SE 434.5) | **+708.4** |

The sign flipped. Deltas on deals 1-7 average roughly +1447; deal 0 is
the outlier that produced Run A's "-4468".

**Why this is also not conclusive:** with `PKR_SUBGAME_ITERS=1`, the
subgame solver performs exactly one CFR iteration — which produces
essentially the uniform strategy at every subgame node. The hook is
replacing the blueprint's river strategy with a near-uniform strategy.
That the delta is +708 (as opposed to +3000 or +5000) is actually
informative: even a *uniform* strategy at river is only ~700 mbb worse
than the blueprint. With 20 iterations the solve should converge to a
strategy meaningfully better than both.

### Run C: 2 deals, 10 inner iterations (in flight at handoff)

Run B suggests the 1-iter solve is doing nothing useful. Run C reruns
with `PKR_SUBGAME_ITERS=10` on 2 deals to see whether the iter budget
moves the delta. If the delta stays positive at 10 iters, the
range-aware approach as wired has a bug beyond the iteration count.
If it flips negative, the wiring is fine and the win was masked by
iteration count.

## Prerequisite: tracker is non-uniform at river

`crates/pkr-subgame/tests/tracker_probe.rs` (ignored by default):
after a scripted limp/call -> check/bet/call -> check/bet/call ->
check/bet line on a fixed board, the tracker's posterior on P0 has:

    uniform = 7.54e-4
    max_p0  = 2.12e-1   (~280x uniform)
    var_p0  = 3.99e-5
    max_p1  = 1.01e-1   (~134x uniform)

If the tracker were degenerate (uniform at river), the hook would
receive uniform and we'd see the old regression. The probe is the
sanity gate for "did the wiring actually thread the posterior".

## Cost — the actual blocker

The uniform version was cheap because the cache key's 8-bin posterior
fingerprint was identical across every call. With a tracked posterior,
the fingerprint differs at every node, and cache hits collapse:

    8-deal run: cache hits 110442, misses 297134
    ~37k hook calls per deal
    ~1.9 ms per miss (10 inner CFR iters; ~0.2 ms at 1 iter)
    => ~2-3 minutes per deal at 1 iter, ~20-30x that at 20 iters

100-deal e2e at 20 iters is ~5 hours. Not routine-A/B viable.

### Mitigations (in priority order)

1. **Coarser-grained solve cache.** Key the cache on a stronger
   fingerprint of the posterior (32-bin, or a MinHash over the top-K
   support). Recovers most hit rate without returning wrong strategies.
   Estimated: 1h.

2. **Fewer hook calls per line.** Solve only at the FIRST river
   decision of each line; deeper river nodes fall back to the
   blueprint. Cuts hook calls ~5-10x. Estimated: 2h.

3. **Parallelize hook solves.** Thread-local subgame contexts.
   Estimated: 4h.

4. **Reduce `PKR_E2E_HANDS`** (linear in solve cost). Env-driven.

5. **Reduce `PKR_SUBGAME_ITERS`** for A/B runs (linear). Env-driven.

## Success criteria status

| criterion | status |
|---|---|
| Wiring verified (probe + hook_debug) | ✅ |
| River e2e delta becomes negative | ⚠️ unresolved (see Run B, C) |
| Control: disabled hook gives delta 0 | ✅ (existing default) |
| Cache correctness with range-aware key | ✅ fingerprint includes range bins |
| Turn extension | ⬜ not attempted |

## What's next

- Wait for Run C (2 deals, 10 iters).
- If Run C is negative: pursue mitigation (1) above to make 100-deal
  runs tractable, then re-measure at 20 iters.
- If Run C is positive: the hook signature is right but the solve
  chain has a bug. Re-read `subgame.rs` `decide` for the range-passing
  path and add a debug assertion that the P1 range is non-uniform
  inside `solve_root_p0_strategy`.
- Runtime integration (`pkr-runtime/src/subgame.rs`) waits on Run C.
- Turn extension waits on the river outcome.
