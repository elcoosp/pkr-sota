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
            // All streets disabled by default. River has a real win
            // (documented in docs/experiments/range-aware-solving-poc.md)
            // ONLY when the caller supplies a tracked, non-uniform
            // opp_range to `decide`. The uniform fallback reproduces
            // the +4836 mbb regression from river-subgame-poc-positive.md.
            //
            // Callers that maintain a RangeTracker should set
            // enabled_streets[3] = true. Anyone without a tracker must
            // leave it false.
            enabled_streets: [false, false, false, false],
        }
    }
}

pub struct SubgameHandle {
    cfg: SubgameConfig,
}

/// Mirror a `GameState` so the acting player becomes seat 0. Used to
/// reuse the P0-only solver for either seat. Swaps per-seat fields;
/// shared fields (pot, board, street) are unchanged.
///
/// SAFETY of the strategy mapping: `action_bucket` computes from
/// `state.actor` + that player's stack/street_bets + opponent's
/// street_bets. After mirroring, `actor=0`, `stacks[0]` is the real
/// actor's stack, `stacks[1]` is the real opponent's. The bucket that
/// the solver uses for concrete action A is the same bucket the real
/// actor would compute. So the returned strategy is directly usable
/// without un-mirroring.
fn mirror_to_seat0(state: &GameState) -> GameState {
    let mut m = state.clone();
    m.stacks.swap(0, 1);
    m.street_bets.swap(0, 1);
    m.total_invested.swap(0, 1);
    m.folded.swap(0, 1);
    m.hole.swap(0, 1);
    m.actor = 1 - m.actor;
    // Bug hunt find: the dealer must swap too. `advance_street_in_place`
    // computes the next street's first actor as `1 - dealer`, so a
    // mirrored state with the wrong dealer would hand the first
    // postflop action to the wrong seat whenever the subgame spans a
    // street boundary. River-only subgames never advance, so the
    // shipped config was safe, but turn subgames (`enabled_streets[2]`)
    // were not.
    m.dealer = 1 - m.dealer;
    m
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
        if state.is_terminal() {
            return None;
        }

        // Subgame solving computes P0's strategy. When the acting player
        // is seat 1, mirror the state so it becomes seat 0.
        let work_state = if state.actor != 0 {
            mirror_to_seat0(state)
        } else {
            state.clone()
        };

        let street_idx = work_state.street as usize;
        if street_idx >= 4 || !self.cfg.enabled_streets[street_idx] {
            return None;
        }

        // Sanity guard (see docs/roadmap/range-aware-solving.md §5).
        //
        // We do NOT assert that opp_range[our_hole] is zero. The
        // RangeTracker documents that card-removal between the two
        // players is not enforced (each player's marginal range is
        // updated independently). Overlap between our hole and the
        // opponent's range is expected; the solver's own `incompatible`
        // check drops those deals when it enumerates.
        //
        // We DO reject an all-zero range: that means the caller passed
        // an uninitialized or degenerate posterior, and the solver would
        // produce a meaningless strategy. The check runs AFTER the
        // street-enabled gate so disabled streets pay nothing.
        let total: f64 = opp_range.iter().sum();
        if total <= 1e-6 {
            return None;
        }

        // Sample hands from the opponent's range and build the P1 range.
        // S10: seed from pot + street + board + our hole, not pot alone
        // (identical pots on different boards sampled the same 8 hands).
        let seed = work_state.pot.to_bits() as u64
            ^ ((work_state.street as u64) << 8)
            ^ work_state.board[..work_state.board_len as usize]
                .iter()
                .fold(0u64, |h, &c| h.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ c as u64)
            ^ our_hole.iter().fold(0u64, |h, &c| h.wrapping_mul(0x100_0000_01b3) ^ c as u64);
        let opp_samples = pkr_subgame::range_tracker::sample_hands_weighted(
            opp_range,
            self.cfg.hands_per_range,
            seed,
        );
        if opp_samples.len() < 2 {
            return None;
        }

        // Always use the caller-supplied hole for P0's range. The
        // mirror swaps positions (stacks, street_bets, actor) but the
        // caller's hole is unchanged by position — it's their actual
        // cards. `mirror_to_seat0` also swaps `hole[0]` and `hole[1]`,
        // but the solver's deal list comes from cfg.p0_range, not from
        // state.hole, so the state-level hole swap is cosmetic.
        let p0_range = Range::weighted(vec![*our_hole], vec![1.0]);
        let p1_range = Range::weighted(
            opp_samples.iter().map(|(h, _)| *h).collect(),
            opp_samples.iter().map(|(_, p)| *p).collect(),
        );

        let cfg = POCConfig {
            root: work_state.clone(),
            p0_range,
            p1_range,
            iterations: self.cfg.iters,
            evaluator: self.cfg.evaluator.as_ref(),
            blueprint: Some((self.cfg.abstraction.as_ref(), self.cfg.table.as_ref())),
        };

        // Solve the subgame and read the root P0 strategy. Since P0's
        // hole is a point mass, the correct reduction is: sum regrets
        // across opponent hands, then regret-match once.
        //
        // SAFE-SOLVE: the raw CFR root can be MORE exploitable than the
        // blueprint (no gadget). safe_solve_root_p0_strategy blends the
        // CFR root toward the blueprint at the largest alpha whose
        // opponent-BR is no worse than the blueprint's — heuristically
        // no more exploitable at the root. Falls back to the raw solve if
        // the blueprint strategy isn't available.
        // Preferred: CFR-D gadget (theorem-backed safety). Falls back to
        // the heuristic blend, then the raw solve, if a step fails.
        pkr_subgame::safe_resolve_gadget(&cfg)
            .or_else(|| pkr_subgame::safe_solve_root_p0_strategy(&cfg))
            .or_else(|| pkr_subgame::solve_root_p0_strategy(&cfg))
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

    /// Read-only access to the underlying blueprint table. Used by
    /// `RuntimeSession` to provide a blueprint fallback path when the
    /// subgame path can't answer.
    pub fn table_ref(&self) -> &CompactRegretTable {
        self.cfg.table.as_ref()
    }
}
