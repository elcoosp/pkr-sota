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
use crate::shadow::Shadow;
use crate::subgame::SubgameHandle;

/// Stateful session that tracks a posterior across a single deal.
///
/// Lifetime `'a` is the abstraction/table/evaluator borrow lifetime.
/// The caller owns those and keeps them alive for the session's whole life.
pub struct RuntimeSession<'a> {
    handle: SubgameHandle,
    tracker: Option<RangeTracker<'a>>,
    /// Phase 1.1: abstract game advanced by translated actions. Present
    /// only when PKR_SHADOW=1. Blueprint hashing + the tracker use it.
    shadow: Option<Shadow>,
    our_seat: u8,
    /// Borrowed once at construction, used to reset the tracker on
    /// every new deal.
    abs: &'a dyn AbstractionBuilder,
    tbl: &'a CompactRegretTable,
    ev: &'a dyn Evaluator,
}

/// §B: enable live action translation (env `PKR_TRANSLATE=1`). Off by
/// default so range tracking is unchanged until it is A/B'd with LBR.
fn translate_observed() -> bool {
    use std::sync::OnceLock;
    static B: OnceLock<bool> = OnceLock::new();
    *B.get_or_init(|| std::env::var("PKR_TRANSLATE").as_deref() == Ok("1"))
}

/// Phase 1.1: enable the shadow-state path (env `PKR_SHADOW=1`).
/// Phase 4: enable vector-form search (env `PKR_VSEARCH=1`).
fn vsearch_enabled() -> bool {
    use std::sync::OnceLock;
    static B: OnceLock<bool> = OnceLock::new();
    *B.get_or_init(|| std::env::var("PKR_VSEARCH").as_deref() == Ok("1"))
}

fn shadow_enabled() -> bool {
    use std::sync::OnceLock;
    static B: OnceLock<bool> = OnceLock::new();
    *B.get_or_init(|| std::env::var("PKR_SHADOW").as_deref() == Ok("1"))
}

impl<'a> RuntimeSession<'a> {
    /// The shadow state if enabled, else None. Callers hash the
    /// blueprint from this when present.
    pub fn shadow_state(&self) -> Option<&GameState> {
        self.shadow.as_ref().map(|s| &s.st)
    }

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
            shadow: None,
            our_seat,
            abs,
            tbl,
            ev,
        }
    }

    /// Start a new deal. Resets the tracker to uniform-over-board.
    pub fn deal_start(&mut self, root: GameState) {
        if shadow_enabled() {
            self.shadow = Some(Shadow::new(root.clone()));
        }
        self.tracker = Some(RangeTracker::new(root, self.abs, self.tbl, self.ev));
    }

    /// Observe an action (ours or the opponent's).
    ///
    /// No-op if no deal is active. Tracker sync failure is logged but
    /// does not abort the caller (state is the caller's ground truth).
    pub fn observe_action(&mut self, action: Action) {
        // Phase 1.1: with a shadow, translate the observed action to the
        // in-tree equivalent and feed THAT to the tracker, so the
        // posterior tracks the strategy the blueprint actually has.
        if self.shadow.is_some() {
            // If this is our own action, apply the EXACT bucket we chose
            // (snapped to the shadow's legal set) rather than running it
            // through the randomized off-tree translation. The prior code
            // routed both sides through `observe_opp`, which uses
            // `translate_bet`'s pseudo-harmonic randomization: an in-tree
            // bet round-trips, but a host clamp or a different
            // bucket->chips convention would randomly snap our own
            // chosen bucket to a neighbouring one, desyncing the shadow
            // state used for subsequent hashing. `observe_own_bucket`
            // existed for exactly this and was dead code.
            if action.player as u8 == self.our_seat {
                let applied = {
                    let sh = self.shadow.as_mut().unwrap();
                    let s = &sh.st;
                    let bucket = pkr_core::abstraction::action_bucket(
                        &action.kind,
                        s.stacks[s.actor],
                        s.street_bets[s.actor],
                        s.street_bets[1 - s.actor],
                        s.pot,
                    );
                    sh.observe_own_bucket(bucket)
                };
                if let Some(t) = self.tracker.as_mut() {
                    let _ = t.apply_action(applied);
                }
                return;
            }
            // Opponent action: translate the off-tree size to the in-tree
            // bucket with the randomized pseudo-harmonic, then advance.
            // Clone the real pre-action state first, then borrow the shadow
            // (borrowck: cannot hold &tracker and &mut shadow at once).
            let real_before = self.tracker.as_ref().map(|t| t.state().clone());
            if let Some(rb) = real_before {
                let applied = {
                    let sh = self.shadow.as_mut().unwrap();
                    sh.observe_opp(&rb, &action, &mut rand::rng())
                };
                if let Some(t) = self.tracker.as_mut() {
                    let _ = t.apply_action(applied);
                }
                return;
            }
        }
        if let Some(t) = self.tracker.as_mut() {
            // §B: when translation is enabled, the posterior update uses
            // the TRANSLATED in-tree bucket for off-tree bet sizes, so
            // the range tracks the strategy the blueprint actually has.
            // The real state still advances with the observed action
            // (correct chips). Default OFF — unchanged behaviour.
            if translate_observed() {
                if let Some(bucket) = crate::translate_live::observed_bet_bucket(
                    t.state(),
                    &action,
                    &mut rand::rng(),
                ) {
                    let _ = t.apply_action_with_bucket(action, bucket as usize);
                    return;
                }
            }
            let _ = t.apply_action(action);
        }
    }

    /// Observe a street advance.
    pub fn observe_street(&mut self, cards: &[u8]) {
        if let Some(sh) = self.shadow.as_mut() {
            sh.advance_street(cards);
        }
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
        // Phase 1.1: with a shadow, solve against the abstract game the
        // blueprint trained on (translated actions), not the real one.
        let st: &GameState = self.shadow_state().unwrap_or(state);
        if st.actor as u8 != self.our_seat {
            return None;
        }
        let opp = 1 - self.our_seat;

        // Phase 4: vector-form search when PKR_VSEARCH=1 and the
        // CURRENT STREET IS ENABLED IN VSearchCfg. Falls through to the
        // sampled/blueprint path on any failure. Ranges are ordered
        // [OOP, IP] for the solver.
        //
        // The prior gate was `st.street != Street::Preflop`, which is a
        // proxy for "postflop", NOT "enabled". Combined with solve_root
        // not checking enabled_streets, a flop state reached the flop
        // tree build and hung. Both are now fixed; this gate is the
        // single place that decides whether to even attempt a solve.
        if vsearch_enabled() {
            let cfg = crate::vsearch::VSearchCfg::default();
            if cfg.enabled_streets[st.street as usize] {
                if let Some(dist) = self.advise_vsearch(st, our_hole) {
                    return Some(dist);
                }
            }
        }

        let opp_range: &[f64; N_HANDS] = tracker.range(opp);
        self.handle.decide(st, our_hole, opp_range)
    }

    /// Vector-search decision: build [OOP,IP] f32 ranges from the tracker,
    /// solve the current street, return the root distribution mapped to
    /// the 6 action buckets.
    fn advise_vsearch(
        &self,
        st: &GameState,
        our_hole: &[u8; 2],
    ) -> Option<[f64; crate::subgame::SUBGAME_BUCKETS]> {
        let tracker = self.tracker.as_ref()?;
        let oop = 1 - st.dealer;
        let r_oop: Vec<f32> = tracker.range(oop as u8).iter().map(|&x| x as f32).collect();
        let r_ip: Vec<f32> = tracker.range((1 - oop) as u8).iter().map(|&x| x as f32).collect();
        // Safety routing: the live bot must use the SAFE solver, not the
        // raw solve.
        //
        // The vector GADGET is UNPROVEN (it scores worse than the blueprint
        // in the on-tree safety tests - see
        // docs/experiments/vector-gadget-safety-status.md). So the live
        // path uses the proven heuristic BOUND. PKR_VSEARCH_BOUND=1 (or the
        // legacy PKR_VSEARCH_GADGET=1, which now maps to the bound) enables
        // it; else the raw solve. A wall-clock cap bounds a live decision.
        let mut cfg = crate::vsearch::VSearchCfg::default();
        cfg.bound = std::env::var("PKR_VSEARCH_BOUND").as_deref() == Ok("1")
            || std::env::var("PKR_VSEARCH_GADGET").as_deref() == Ok("1");
        // Never enable the unproven vector gadget in the live path.
        cfg.gadget = false;
        cfg.time_budget_ms = std::env::var("PKR_VSEARCH_BUDGET_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2000);
        let ranges = || [r_oop.clone(), r_ip.clone()];
        let acts = if cfg.bound {
            crate::vsearch::solve_root_bounded(st, ranges(), *our_hole, &cfg, self.abs, self.tbl)
                .or_else(|| crate::vsearch::solve_root(st, ranges(), *our_hole, &cfg))?
        } else {
            crate::vsearch::solve_root(st, ranges(), *our_hole, &cfg)?
        };

        // Map each (Act, prob) to an action bucket; a concrete action's
        // bucket comes from action_bucket on a representative kind.
        let mut out = [0.0f64; crate::subgame::SUBGAME_BUCKETS];
        let mut total = 0.0;
        for (act, p) in &acts {
            use pkr_subgame::vector::Act;
            let kind = match act {
                Act::Fold => pkr_core::state::ActionKind::Fold,
                Act::Check => pkr_core::state::ActionKind::Check,
                Act::Call => pkr_core::state::ActionKind::Call,
                Act::Bet(t) => pkr_core::state::ActionKind::Bet(*t),
            };
            let b = pkr_core::abstraction::action_bucket(
                &kind, st.stacks[st.actor], st.street_bets[st.actor],
                st.street_bets[1 - st.actor], st.pot) as usize;
            if b < crate::subgame::SUBGAME_BUCKETS {
                out[b] += *p as f64;
                total += *p as f64;
            }
        }
        if total > 1e-9 {
            for x in out.iter_mut() { *x /= total; }
            return Some(out);
        }
        None
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

impl<'a> RuntimeSession<'a> {
    /// Advise with automatic fallback to the blueprint.
    ///
    /// If the subgame path can answer (our turn, enabled street), returns
    /// the subgame-solved strategy. Otherwise reads the blueprint's
    /// average strategy at the current infoset hash.
    ///
    /// `blueprint_hash` must be precomputed by the caller — this struct
    /// deliberately does not know how to compute infoset hashes; that
    /// stays in the caller's abstraction layer.
    ///
    /// Returns `None` only if both paths fail (e.g. unknown infoset hash
    /// AND disabled street). Callers that always have a blueprint path
    /// should pass a hash they know is valid, in which case this method
    /// never returns None.
    pub fn advise_or_blueprint(
        &self,
        state: &GameState,
        our_hole: &[u8; 2],
        blueprint_hash: u64,
    ) -> Option<[f64; crate::subgame::SUBGAME_BUCKETS]> {
        if let Some(s) = self.advise(state, our_hole) {
            return Some(s);
        }
        let mut raw = [0.0f32; crate::subgame::SUBGAME_BUCKETS];
        self.handle_blueprint_strategy(blueprint_hash, &mut raw);
        let sum: f32 = raw.iter().sum();
        if sum <= 1e-12 {
            // Blueprint has no data at this infoset — fall back to uniform.
            let n = crate::subgame::SUBGAME_BUCKETS as f64;
            let mut u = [0.0f64; crate::subgame::SUBGAME_BUCKETS];
            for x in u.iter_mut() {
                *x = 1.0 / n;
            }
            return Some(u);
        }
        let mut out = [0.0f64; crate::subgame::SUBGAME_BUCKETS];
        for i in 0..crate::subgame::SUBGAME_BUCKETS {
            out[i] = (raw[i] / sum) as f64;
        }
        Some(out)
    }

    /// Read the blueprint average strategy. Exposed so callers can build
    /// their own fallback pipeline.
    pub fn blueprint_strategy(&self, hash: u64) -> [f64; crate::subgame::SUBGAME_BUCKETS] {
        let mut raw = [0.0f32; crate::subgame::SUBGAME_BUCKETS];
        self.handle_blueprint_strategy(hash, &mut raw);
        let mut out = [0.0f64; crate::subgame::SUBGAME_BUCKETS];
        let sum: f32 = raw.iter().sum();
        if sum <= 1e-12 {
            return out;
        }
        for i in 0..crate::subgame::SUBGAME_BUCKETS {
            out[i] = (raw[i] / sum) as f64;
        }
        out
    }

    fn handle_blueprint_strategy(&self, hash: u64, out: &mut [f32; crate::subgame::SUBGAME_BUCKETS]) {
        self.handle.table_ref().get_average_strategy_into(hash, out);
    }
}
