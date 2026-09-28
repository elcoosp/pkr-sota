# Range-aware subgame solving — design doc

**Date:** 2026-09-27
**Status:** wiring LANDED (commits 461e67c, 0b06058); e2e outcome UNRESOLVED
**Motivation:** The uniform-range subgame hook regresses e2e by +4836 mbb
(see `docs/experiments/river-subgame-poc-positive.md`). Fixing it requires
threading the opponent posterior through the BR walk.

## Outcome (2026-09-28)

The `Traversal` refactor and the `SubgameHook::strategy` signature
change described below are **landed**.

**Wiring verified.** `crates/pkr-subgame/tests/tracker_probe.rs` proves
the tracker's posterior at a river decision has max mass ~280x uniform.
The hook receives it; the solve sees a real range.

**E2E outcome unresolved.**
- 1 deal, 1 subgame iter: delta **-4468** — noise (1-deal in-sample
  BR is dominated by overfit; blueprint alone reads 8981 at 1 deal vs
  2414 at 8).
- 8 deals, 1 subgame iter: delta **+708 ± 434** — sign flipped.
  With one inner CFR iteration the subgame strategy is essentially
  uniform.
- 2 deals, 10 inner iters: in flight at handoff.

**The blocking work is cost + iteration budget.** At 20 inner iters,
100-deal e2e is ~5h. The range fingerprint defeats the solve cache
(each river node sees a distinct posterior).

See `docs/experiments/range-aware-solving-poc.md` for the full record
and the mitigation list.

## The problem, in one paragraph

`SubgameHook::strategy(state, hero_hole, hero_is_p0)` has no access to
the opponent's range. `SubgameHandle::decide` falls back to a uniform
prior. When the opponent is the blueprint (as in an e2e eval), its
actual range at river nodes is heavily conditioned on its own prior
street play. The solve optimizes P0 for the wrong opponent and produces
a strategy worse than the blueprint's own. To fix, the hook needs the
current opponent posterior at every agent decision node.

## Prerequisites (done)

- `RangeTracker` maintains a posterior over both players' hands through
  a full betting line (verified).
- `RangeTracker::undo_last_action` exists (commit d6eede6).
- `RangeTracker::sample_hands_weighted` returns the true posterior mass.
- `Solver` accepts ranges as `Range::weighted` (verified).
- `SubgameHandle` handles either seat via `mirror_to_seat0`.

## API changes

### 1. `SubgameHook` signature

Current:
~~~rust
fn strategy(
    &self,
    state: &GameState,
    hero_hole: &[u8; 2],
    hero_is_p0: bool,
) -> Option<[f64; K]>
~~~

New:
~~~rust
fn strategy(
    &self,
    state: &GameState,
    hero_hole: &[u8; 2],
    hero_is_p0: bool,
    opp_range: &[f64; 1326],
) -> Option<[f64; K]>
~~~

`opp_range` is the tracker's current posterior over the opponent's
hole cards, indexed by combinadic pair index.

### 2. `subgame.rs` uses `opp_range` instead of uniform

In `SubgameHandle::decide`:

~~~rust
// Before:
let p1_range = Range::uniform(opp_samples.iter().map(|(h, _)| *h).collect());

// After:
let p1_range = Range::weighted(
    opp_samples.iter().map(|(h, _)| *h).collect(),
    opp_samples.iter().map(|(_, p)| *p).collect(),
);
~~~

`sample_hands_weighted` needs to accept the caller-supplied range.
Currently it takes the tracker's range as its first argument; we
pass the tracker's range directly. No change needed.

### 3. RangeTracker threaded through `walk_fixed`

`walk_fixed` currently takes `(&mut GameState, ..., ranks)`. We add
`tracker: &mut RangeTracker` to the arg list and sync mutations:

~~~rust
// In walk_fixed, before every state.apply_action_in_place / advance_street:
tracker.apply_action(action);
// After every state.undo_action:
tracker.undo_last_action();
~~~

Critically, `maybe_advance` in walk_fixed already uses `advance_street_in_place`
and undo. Both need tracker mirroring. Options:

**Option A** — inline the tracker sync into `maybe_advance`'s caller.
Every `if advanced { state.undo_action(); }` becomes
`if advanced { state.undo_action(); tracker.undo_last_action(); }`.

**Option B** — wrap `state` and `tracker` into a small `Traversal`
struct that exposes `apply_action`, `undo_action`, `advance_street`
as atomic operations. Cleaner but a bigger refactor.

Recommendation: **Option B**. The `maybe_advance` + undo pattern is
already fragile; wrapping it once removes a class of bugs.

### 4. Tracker construction per deal

`sampled_br_one_seat` builds a `RangeTracker` per deal from a fresh
preflop root, then walks. Two lifecycle options:

**Option 1** — one tracker per deal, reset per BR iteration.
Simple but reconstructs `Range` arrays 8x per BR call.

**Option 2** — one tracker per BR iteration, reset between deals.
`RangeTracker::reset(root)` restores the initial uniform prior.
Reuses the allocations. Preferred.

Add `RangeTracker::reset(&mut self, root: GameState)` that clears
`undo_stack`, sets `state = root`, and re-initializes ranges to
uniform-excluding-board.

### 5. Cache key must include range state

The e2e test caches solves by `(board, hole, history_signature)`.
Once ranges matter, two calls with the same key but different
`opp_range` would return the wrong cached strategy.

Two options:

**Option A** — hash the range into the key (fast, but collisions
  possible).
**Option B** — skip caching entirely for range-aware; solves are
  naturally distinct per position. Slower but correct.

Recommendation: **Option A** with a 64-bit hash of the range. The
range is a 1326-vector of f64; hashing it is ~10us. If two ranges
differ enough to matter, their hashes differ.

## Test plan

### Unit tests

1. **`RangeTracker::reset`** — after `reset(root)`, ranges are uniform
   excluding the board, and `undo_stack` is empty.
2. **Undo symmetry** — apply action, undo, verify ranges identical to
   pre-apply state. Do this on a small tree (5 actions deep).
3. **Streets restore** — apply action, advance street, undo, advance,
   undo. Verify ranges restore to each checkpoint.

### Integration tests

4. **Range at river** — play a scripted line, verify the tracker's
   opponent posterior has the expected shape (aggressive lines →
   stronger hands upweighted).
5. **Range-aware solve is non-uniform** — run the same solve with
   uniform vs tracked range; verify the returned strategy differs.
6. **E2E** — the river subgame e2e test with range-aware hook. If the
   delta vs blueprint flips to negative, the fix works.

### Failure modes to watch for

- **Tracker diverges from GameState.** If any `state.apply_action_in_place`
  in `walk_fixed` doesn't have a matching `tracker.apply_action`, ranges
  drift. Mitigated by the `Traversal` wrapper (Option B above).
- **Cache collisions.** Hashing a 1326-vector to 64 bits — birthday
  collisions at ~2^32 distinct ranges. Not a practical issue for
  eval-scale runs.
- **Range support excludes the hero.** If the tracker's range ever
  contains the hero's hole, `sample_hands_weighted` will return
  overlapping hands and the solver's `incompatible` check will drop
  those deals. Silent undercounting. Add a debug_assert that
  `tracker.range(1-actor)` has zero mass on `hero_hole`.

## Estimated effort

| Step | Hours |
|---|---|
| `Traversal` wrapper (Option B) | 1.5 |
| `RangeTracker::reset` + tests | 0.5 |
| `SubgameHook` signature + threading | 1.0 |
| `subgame.rs` range-aware decide | 0.5 |
| e2e re-run + debug | 1.0 |
| **Total** | **4.5** |

## Success criteria

1. **River-only e2e delta becomes negative.** The uniform version gave
   +4836 mbb. Range-aware should give a negative delta, magnitude
   unknown.
2. **No regression when opponent plays uniform.** Control test: hook
   with `enabled_streets = [false, false, false, false]` gives
   delta = 0.0.
3. **Cache is correct.** Same (board, hole, history, range) returns
   the same strategy; different range returns different strategy.

## What to do when this works

- Enable `enabled_streets = [false, false, false, true]` (river only)
  as default. River is 40% of postflop value at 1/10 the latency
  cost of turn.
- Ship the runtime integration. Update `docs/roadmap/subgame-solving-plan.md`.
- Then extend to turn with the same pattern (turn adds chance nodes).

## Files to modify

- `crates/pkr-exploit/src/best_response.rs` — SubgameHook trait,
  Traversal struct, walk_fixed signature, sampled_br_one_seat
- `crates/pkr-subgame/src/range_tracker.rs` — reset method
- `crates/pkr-runtime/src/subgame.rs` — decide uses opp_range param
- `crates/pkr-exploit/tests/e2e_subgame_hook.rs` — pass tracker ranges
- `crates/pkr-exploit/tests/street_decomposition.rs` — ForceFoldHook
  needs updated signature

## Related

- `docs/experiments/river-subgame-poc-positive.md` — the regression
  that motivated this
- `docs/roadmap/subgame-solving-plan.md` — the 2-week build plan
- Commit d6eede6 — RangeTracker::undo_last_action (prerequisite)
