# Solve first river node only — design

**Date:** 2026-09-28
**Status:** Proposed (not started)
**Motivation:** `docs/experiments/range-aware-solving-poc.md` §Cost.
Range-aware solving is correct but ~37k hook calls/deal. Most of those
calls are at *deep* river nodes where the tracker's posterior has had
its full information extracted already. Solving every node is wasteful.

## The idea

The subgame solve is needed at a node if and only if the blueprint's
abstraction is likely wrong there. On the river that means:

- **First river decision of a line** (the node at which the river
  subgame begins, i.e. the first time `state.street == River` with
  `state.actor == our_seat`). The blueprint has never seen the turn
  action that got us here concretely; its river strategy is a
  coarse-abstraction approximation.
- **Subsequent river decisions.** The blueprint's river play past the
  first decision is *also* a coarse abstraction, but the tracker's
  posterior has fewer degrees of freedom left by then. Empirically, the
  extra solves buy little.

## How to detect "first river decision of a line"

At each agent node in the walker:

    is_first_river_decision =
        state.street == River
        && !state.river_decisions_made[our_seat]  // count per deal
        && state.actor == our_seat

Because the walker is per deal and does apply/undo on a single
GameState, the count is a per-deal field on the walker's local state,
not on GameState. It's incremented on the *first* river decision in
`walk_fixed` (and in `collect_cfv`) and decremented on undo.

## Hook interface

Two options:

**Option A — hook decides.** The hook receives the full state and can
itself decide "solve vs fallback" by looking at `state.street_bets`.
But `street_bets` doesn't tell us whether it's the first decision;
the FIRST decision is the one where the *previous* street transition
happened at a specific street_bet == 0 state. Fragile.

**Option B — walker tells the hook.** Add a boolean to the hook call:

    fn strategy(
        &self,
        state: &GameState,
        hero_hole: &[u8; 2],
        hero_is_p0: bool,
        opp_range: &[f64; N_HANDS],
        river_node_index: u32,   // 0 for first river decision
    ) -> Option<[f64; K]>;

Then the hook can decide: `if river_node_index == 0 { solve } else { None }`.
The walker is the only place that knows the index (it owns the
per-deal state through apply/undo).

Option B is cleaner. It also lets the hook implement any *policy*
about which nodes to solve, without further walker changes.

## Measured payoff (2026-09-28, updated)

**The 10x estimate was wrong.** Counter run (8 deals, 10 iters,
`PKR_COUNT_RIVER_NODES=1`):

    river nodes: shallow=198873 deep=215168 (48.0% shallow)

So eliminating all "deep" river hook calls saves ~52% of hook calls,
i.e. a **~2x speedup**, not 10x.

The counter classifies "shallow" as `street_bets[0] == 0 &&
street_bets[1] == 0` — no river money committed yet by anyone. This is
a proxy for "first decision of the line", but not exact: after a
check-check river branch, a subsequent node can also have zero
street_bets if the walker undoes back. In practice it's close enough.

**Given the modest payoff and the unresolved magnitude of the
underlying e2e effect, this mitigation is now deprioritized.**
See `docs/experiments/range-aware-solving-poc.md` Run F for the
reasoning: the effect is small enough that reducing cost does not
change the decision to ship.

## Interaction with the cache

The cache currently keys on `(board, hole, history)`. Adding the
"first-river-only" policy doesn't change the key; it just means deeper
river nodes never call the hook, so the cache never sees them.

## How to validate

A/B at 8 deals, 10 iters:

- Run D (all river nodes): delta -53.2 mbb, wall 346s
- Run X (first river node only): measure delta and wall
- Success criterion: delta stays within ±1 SE of Run D, wall drops >=3x

If delta stays in [-200, +100] with wall <= 120s, this is the
shipping configuration. If delta blows up, the deeper river nodes
were doing real work and the mitigation must be selective.

## Not doing (yet)

- Turn extension. Same pattern but adds chance nodes. Wait until river
  has a stable cost profile.
- Parallelize hook solves. Higher effort for similar payoff.
