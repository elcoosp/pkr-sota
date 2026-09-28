//! Runtime session — a stateful wrapper over `SubgameHandle` that owns
//! a `RangeTracker` and mirrors game events into it.
//!
//! See `docs/roadmap/runtime-tracker-integration.md`.
//!
//! # Usage
//!
//! ```ignore
//! let mut session = RuntimeSession::new(handle, our_seat, abs, tbl, ev);
//! session.deal_start(root_state);
//! // On every action (ours or opponent's):
//! session.observe_action(action);
//! // On every street advance:
//! session.observe_street(&cards);
//! // At our decision point:
//! if let Some(strategy) = session.advise(&state, &our_hole) {
//!     // use strategy
//! }
//! ```
//!
//! The runtime does not leak `RangeTracker` details to bot callers.

use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{Action, GameState};
use pkr_subgame::range_tracker::{N_HANDS, RangeTracker};
use crate::subgame::SubgameHandle;

/// Stateful session that tracks a posterior across a single deal.
///
/// Lifetime `'a` is the abstraction/table/evaluator borrow lifetime.
/// The caller owns those and keeps them alive for the session's whole life.
pub struct RuntimeSession<'a> {
    handle: SubgameHandle,
    tracker: Option<RangeTracker<'a>>,
    our_seat: u8,
    /// Borrowed once at construction, used to reset the tracker on
    /// every new deal.
    abs: &'a dyn AbstractionBuilder,
    tbl: &'a CompactRegretTable,
    ev: &'a dyn Evaluator,
}

impl<'a> RuntimeSession<'a> {
    /// Create a session for `our_seat` (0 or 1).
    pub fn new(
        handle: SubgameHandle,
        our_seat: u8,
        abs: &'a dyn AbstractionBuilder,
        tbl: &'a CompactRegretTable,
        ev: &'a dyn Evaluator,
    ) -> Self {
        assert!(our_seat < 2, "seat must be 0 or 1");
        RuntimeSession {
            handle,
            tracker: None,
            our_seat,
            abs,
            tbl,
            ev,
        }
    }

    /// Start a new deal. Resets the tracker to uniform-over-board.
    pub fn deal_start(&mut self, root: GameState) {
        self.tracker = Some(RangeTracker::new(root, self.abs, self.tbl, self.ev));
    }

    /// Observe an action (ours or the opponent's).
    ///
    /// No-op if no deal is active. Tracker sync failure is logged but
    /// does not abort the caller (state is the caller's ground truth).
    pub fn observe_action(&mut self, action: Action) {
        if let Some(t) = self.tracker.as_mut() {
            let _ = t.apply_action(action);
        }
    }

    /// Observe a street advance.
    pub fn observe_street(&mut self, cards: &[u8]) {
        if let Some(t) = self.tracker.as_mut() {
            let _ = t.advance_street(cards);
        }
    }

    /// Ask for a strategy at the current decision point.
    ///
    /// Returns `None` when:
    /// - No deal is active.
    /// - The current actor is not `our_seat`.
    /// - The current street is not in `enabled_streets`.
    /// - The subgame solve fails.
    ///
    /// Callers fall back to the blueprint on `None`.
    pub fn advise(
        &self,
        state: &GameState,
        our_hole: &[u8; 2],
    ) -> Option<[f64; crate::subgame::SUBGAME_BUCKETS]> {
        let tracker = self.tracker.as_ref()?;
        if state.actor as u8 != self.our_seat {
            return None;
        }
        let opp = 1 - self.our_seat;
        let opp_range: &[f64; N_HANDS] = tracker.range(opp);
        self.handle.decide(state, our_hole, opp_range)
    }

    /// Current opponent posterior, for diagnostics / tests.
    pub fn opp_range(&self) -> Option<&[f64; N_HANDS]> {
        let opp = 1 - self.our_seat;
        self.tracker.as_ref().map(|t| t.range(opp))
    }

    /// Whether a deal is currently being tracked.
    pub fn is_active(&self) -> bool {
        self.tracker.is_some()
    }
}
