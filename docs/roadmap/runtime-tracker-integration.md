# Runtime tracker integration — design

**Date:** 2026-09-28
**Status:** Proposed, not started.
**Blocked by:** there is no runtime bot binary yet. Only `pkr-trainer`
exists, which never plays a game — it trains a blueprint and evaluates
it. Integration becomes actionable when a bot consumer lands.

## The gap

`SubgameHandle::decide(state, our_hole, opp_range)` requires the
opponent posterior. Producing it requires a `RangeTracker`. Neither
exists on any code path that plays a live game.

## Why not just expose `RangeTracker` to bot callers

`RangeTracker<'a>` borrows `&'a dyn AbstractionBuilder`,
`&'a CompactRegretTable`, and `&'a dyn Evaluator`. A caller cannot
hold it in a struct that owns the Arcs without self-referential
borrow-checker pain. The tracker also needs a `deal_start` /
`observe_action` lifecycle that the caller would have to remember.

## Proposed `RuntimeSession` (owned-Arc wrapper)

The cleanest shape is to give `RuntimeSession<'a>` the lifetime of the
caller's Arcs. The bot constructs the tracker's dependencies once at
startup and holds them for the process lifetime. Then:

```rust
pub struct RuntimeSession<'a> {
    handle: SubgameHandle,
    tracker: RangeTracker<'a>,
    our_seat: usize,
}

impl<'a> RuntimeSession<'a> {
    pub fn new(
        handle: SubgameHandle,
        root: GameState,
        abs: &'a dyn AbstractionBuilder,
        tbl: &'a CompactRegretTable,
        eval: &'a dyn Evaluator,
        our_seat: usize,
    ) -> Self {
        RuntimeSession {
            tracker: RangeTracker::new(root, abs, tbl, eval),
            handle,
            our_seat,
        }
    }

    pub fn deal_start(&mut self, root: GameState) {
        self.tracker = /* fresh tracker on root */;
    }

    pub fn observe_action(&mut self, a: Action) {
        let _ = self.tracker.apply_action(a);
    }

    pub fn observe_street(&mut self, cards: &[u8]) {
        let _ = self.tracker.advance_street(cards);
    }

    pub fn advise(&self, state: &GameState, our_hole: &[u8; 2]) -> Option<[f64; 6]> {
        if state.actor != self.our_seat {
            return None;
        }
        let opp = 1 - self.our_seat;
        self.handle.decide(state, our_hole, self.tracker.range(opp as u8))
    }
}
