# Phase 4.3 design note (2026-10-10)

## The gap

`RangeTracker::set_range` exists (`crates/pkr-subgame/src/range_tracker.rs:297`)
with a docstring that says "Used after a search solve to set our own range
to the strategy we actually played." **No caller uses it.** That is
correct for the current shipping path (river-only) and wrong for any
build that enables `PKR_VSEARCH_STREETS=t` or `tf`.

## Why it is broken when search runs

`observe_action` for our own action goes through
`RangeTracker::apply_action` → `update_range_for_action` →
`update_range_for_bucket` → `get_average_strategy_into`. That reads the
**blueprint** strategy at the current infoset. When `advise_vsearch`
returned a **search** strategy, our own range is updated with the
*blueprint's* view of what we would have done — not the search's view.
This mismatch only matters if a later street re-solves against our
range. Live default is river-only, so there is no later street.

## Why a one-line fix does not exist

The plan's `advise()` signature is
`fn advise(&self, state, our_hole) -> Option<[f64; SUBGAME_BUCKETS]>`.
This returns a single bucket distribution for **our specific hole only**.
A correct 4.3 update needs, for every hand h in our 1326-hand range,
`P(we chose bucket b | h)`. The vector solver already produces this
(`Solver::root_strategy_all` returns `Vec<Vec<f32>>`, `n_actions × 1326`),
but the session throws it away after collapsing to the aggregate.

## Design options

1. **Extend `advise` to return per-hand.** Change the return type (or
   add `advise_per_hand`) to carry `Option<Vec<[f64; SUBGAME_BUCKETS]>>`
   of length 1326. Store the result on the session for the next
   `observe_action`. Consumer must handle `None` (blueprint fallback).
   Breaking API change for `advise_or_blueprint` and its callers.

2. **Add a parallel `advise_traced`** that returns both the aggregate
   and the per-hand strategy. Same storage requirement; does not break
   existing callers.

3. **Accept the mismatch and document.** Update the tracker with the
   aggregate distribution (a crude approximation: treat our range as
   uniform over bucket choice for non-our-hole entries). Not correct,
   but better than the current silent blueprint mismatch if search
   ever runs.

Recommended: option 1 when turn/flop search is enabled. Until then,
leave `set_range` uncalled and treat this as a known limitation.

## Acceptance test when this lands

For a river state reached from a known preflop/flop/turn history with
`PKR_VSEARCH_STREETS=tr`:
1. Play one hand with the search on.
2. After the search decision, snapshot `tracker.range(our_seat)`.
3. Compare against a hand-computed update using the search strategy
   for the chosen bucket; assert elementwise `< 1e-6` after
   renormalization.
4. Run `lbr_eval --search` and confirm the reading is not worse than
   without 4.3 (removing the mismatch should help; if not, the search
   strategy is not being consumed anywhere downstream either).
