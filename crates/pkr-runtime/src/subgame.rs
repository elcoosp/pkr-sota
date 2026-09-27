//! Subgame-solving extension for the runtime.
//!
//! `SolverHandle` (see `lookup.rs`) answers infoset-hash queries from a
//! precomputed blueprint. That path is fast, stateless, and correct for
//! every street.
//!
//! `SubgameHandle` is the optional extension: given the current concrete
//! `GameState`, the acting player's hole cards, and a posterior over the
//! opponent's hole cards, it runs a small CFR solve on the current
//! subgame and returns the strategy at the root. The bot uses this on
//! turn and river to escape the blueprint's abstraction on those streets.
//!
//! # Design
//!
//! Subgame solving is *not* a drop-in replacement for `get_advice_fast`.
//! The blueprint path only needs a hash; the solve path needs the whole
//! state plus a range. So `SubgameHandle` exposes a different API:
//!
//! ```ignore
//! let handle = SubgameHandle::new(cfg);
//! if let Some(strategy) = handle.decide(&state, &our_hole, &opp_range) {
//!     // use strategy at the current decision point
//! } else {
//!     // subgame disabled for this street or state; fall back to blueprint
//! }
//! ```
//!
//! # When it returns `None`
//!
//! - Street is not in `cfg.enabled_streets`.
//! - State is terminal or the actor is not `our_player`.
//! - No legal actions at the current node.
//! - Subgame solve fails or times out.
//!
//! In every case the caller falls back to the blueprint path.

use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::GameState;
use pkr_subgame::range_tracker::N_HANDS;
use pkr_subgame::{POCConfig, Range};
use std::sync::Arc;

/// Which streets the handle will solve. Preflop and flop are always
/// `false` in practice — the subgame tree doesn't scale that far.
pub const SUBGAME_BUCKETS: usize = 6;

#[derive(Clone)]
pub struct SubgameConfig {
    pub evaluator: Arc<dyn Evaluator>,
    pub abstraction: Arc<dyn AbstractionBuilder>,
    pub table: Arc<CompactRegretTable>,
    /// CFR iterations per solve. 25 is enough for turn; 50 for river.
    pub iters: u32,
    /// How many hands to sample from each player's posterior. 8-12 typical.
    pub hands_per_range: usize,
    /// Which streets get subgame solved: [preflop, flop, turn, river].
    /// Default `[false, false, true, true]`.
    pub enabled_streets: [bool; 4],
}

impl Default for SubgameConfig {
    fn default() -> Self {
        SubgameConfig {
            evaluator: Arc::new(pkr_eval::NlheEvaluator),
            abstraction: Arc::new(pkr_abstraction::KMeansAbstraction::new(
                vec![],
                Arc::new(pkr_eval::NlheEvaluator),
            )),
            table: Arc::new(CompactRegretTable::with_capacity(1)),
            iters: 25,
            hands_per_range: 8,
            enabled_streets: [false, false, true, true],
        }
    }
}

pub struct SubgameHandle {
    cfg: SubgameConfig,
}

impl SubgameHandle {
    pub fn new(cfg: SubgameConfig) -> Self {
        SubgameHandle { cfg }
    }

    /// Solve the subgame rooted at `state` and return the strategy at the
    /// current decision point, given our hole cards and a posterior over
    /// the opponent's hands.
    ///
    /// Returns `None` when the street isn't enabled, the state isn't at a
    /// decision point, or the solve doesn't produce a usable strategy.
    pub fn decide(
        &self,
        state: &GameState,
        our_hole: &[u8; 2],
        opp_range: &[f64; N_HANDS],
    ) -> Option<[f64; SUBGAME_BUCKETS]> {
        let street_idx = state.street as usize;
        if street_idx >= 4 || !self.cfg.enabled_streets[street_idx] {
            return None;
        }
        if state.is_terminal() {
            return None;
        }
        // Subgame solving computes P0's strategy. If it's not P0's turn,
        // the root is either a P1 decision or a chance node, and reading
        // sum0 gives zeros. Require the caller to pass the state at
        // P0-to-act.
        if state.actor != 0 {
            return None;
        }

        // Sample hands from the opponent's range and build the P1 range.
        let opp_samples = pkr_subgame::range_tracker::sample_hands_weighted(
            opp_range,
            self.cfg.hands_per_range,
            state.pot.to_bits() as u64,
        );
        if opp_samples.len() < 2 {
            return None;
        }

        // Our range is a point mass on our concrete hand.
        let p0_range = Range::weighted(vec![*our_hole], vec![1.0]);
        let p1_range = Range::weighted(
            opp_samples.iter().map(|(h, _)| *h).collect(),
            opp_samples.iter().map(|(_, p)| *p).collect(),
        );

        let cfg = POCConfig {
            root: state.clone(),
            p0_range,
            p1_range,
            iterations: self.cfg.iters,
            evaluator: self.cfg.evaluator.as_ref(),
            blueprint: Some((self.cfg.abstraction.as_ref(), self.cfg.table.as_ref())),
        };

        // Pull the strategy that CFR learned for our concrete hand at
        // the root. Since P0 range is a point mass on `our_hole`, the
        // first deal in the solver corresponds to (our_hole, first
        // opponent sample). The root strategy depends on our hole, which
        // is fixed across all deals — so we average over opponent samples.
        let result = pkr_subgame::root_strategies(&cfg);
        if result.strategies.is_empty() {
            return None;
        }

        // Average over deals (which vary only in opponent hand).
        let mut avg = [0.0f64; SUBGAME_BUCKETS];
        for (i, s) in result.strategies.iter().enumerate() {
            let w = opp_samples.get(i).map(|(_, p)| *p).unwrap_or(1.0);
            for b in 0..SUBGAME_BUCKETS {
                avg[b] += w * s[b];
            }
        }
        let sum: f64 = avg.iter().sum();
        if sum <= 1e-9 {
            return None;
        }
        for b in 0..SUBGAME_BUCKETS {
            avg[b] /= sum;
        }
        Some(avg)
    }

    /// Convenience: same as `decide` but returns the index of the highest-
    /// probability action. Mirrors `get_advice_fast`'s "fast" semantics.
    pub fn decide_argmax(
        &self,
        state: &GameState,
        our_hole: &[u8; 2],
        opp_range: &[f64; N_HANDS],
    ) -> Option<usize> {
        let s = self.decide(state, our_hole, opp_range)?;
        let mut best = 0usize;
        let mut best_p = -1.0;
        for (i, &p) in s.iter().enumerate() {
            if p > best_p {
                best_p = p;
                best = i;
            }
        }
        Some(best)
    }

    /// True if this street is enabled for subgame solving.
    pub fn street_enabled(&self, street_idx: usize) -> bool {
        street_idx < 4 && self.cfg.enabled_streets[street_idx]
    }
}
