# Runtime tracker integration — design

**Date:** 2026-09-28
**Status:** SHIPPED 2026-09-29 — `RuntimeSession` in
`crates/pkr-runtime/src/session.rs` (re-exported at crate root).
Still blocked on a bot consumer binary; `pkr-trainer` does not play
games.

## What shipped (2026-09-29)

    use pkr_runtime::RuntimeSession;

    let mut session = RuntimeSession::new(handle, our_seat, abs, tbl, ev);
    session.deal_start(root);
    session.observe_action(action);
    session.observe_street(&cards);
    let strat = session.advise_or_blueprint(&state, &hole, hash);

Three tests, all passing:
- `session_smoke.rs::session_owns_tracker_and_updates`
- `session_smoke.rs::session_deal_start_resets_tracker`
- `session_smoke.rs::advise_or_blueprint_always_returns_a_strategy`
- plus `advise_or_blueprint` vs `blueprint_strategy` divergence
  (proves the subgame path actually fires).

Runnable example: `crates/pkr-runtime/examples/bot_loop.rs` —
`cargo run --release -p pkr-runtime --example bot_loop`.

The `advise_or_blueprint` variant guarantees a normalized strategy on
every call: subgame if the street is enabled and it is our turn,
blueprint average otherwise, uniform as a final fallback. `advise`
(no fallback) returns `None` in the fallback cases, for callers who
want to handle them explicitly.

The section below is kept as the original design for reference. The
shipped API matches it in shape (borrow-based, no `Arc`-in-tracker
refactor).

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
