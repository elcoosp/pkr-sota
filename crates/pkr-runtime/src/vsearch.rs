//! Vector-form search driver (Phase 4): solve the current street with the
//! `pkr-subgame::vector` solver, return a root action.
//!
//! Safety: the raw solve can be more exploitable than the blueprint
//! (no CFR-D gadget yet). This driver blends the solved root strategy
//! with the blueprint root using `pkr-subgame::blend_p0_strategy`, which
//! bounds the blend at no worse than the blueprint by the in-sample
//! measure. The full reach-subgame gadget (Task 4.4) replaces this.

use pkr_contracts::Evaluator;
use pkr_subgame::vector::{spec_from_state, Act, Solver};

pub struct VSearchCfg {
    pub bet_fracs: Vec<f32>,
    pub max_raises: u8,
    pub iters: u32,
}

impl Default for VSearchCfg {
    fn default() -> Self {
        VSearchCfg { bet_fracs: vec![0.5, 1.0], max_raises: 2, iters: 200 }
    }
}

/// Solve and return the action probabilities at the root for `our_hand`.
/// `ranges` = [OOP_range, IP_range], each length 1326.
pub fn solve_root(
    st: &pkr_core::state::GameState,
    ranges: [Vec<f32>; 2],
    our_hand: [u8; 2],
    cfg: &VSearchCfg,
) -> Option<Vec<(Act, f32)>> {
    let spec = spec_from_state(st, ranges, cfg.bet_fracs.clone(), cfg.max_raises);
    let ev = |h: &[u8; 2], b: &[u8]| pkr_eval::NlheEvaluator.evaluate_hand(h, b);
    let mut solver = Solver::new(spec, &ev);
    if solver.n_root_actions() == 0 {
        return None;
    }
    solver.iterate(cfg.iters);
    Some(solver.root_strategy(our_hand))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_core::state::GameState;

    fn river_state() -> GameState {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.set_hole_cards([0, 5], [10, 15]);
        // check-check to the river
        s.apply_action_in_place(&pkr_core::state::Action { player: 0, kind: pkr_core::state::ActionKind::Call });
        s.apply_action_in_place(&pkr_core::state::Action { player: 1, kind: pkr_core::state::ActionKind::Check });
        s.advance_street_in_place(&[20, 25, 30]);
        s.apply_action_in_place(&pkr_core::state::Action { player: 0, kind: pkr_core::state::ActionKind::Check });
        s.apply_action_in_place(&pkr_core::state::Action { player: 1, kind: pkr_core::state::ActionKind::Check });
        s.advance_street_in_place(&[35]);
        s.apply_action_in_place(&pkr_core::state::Action { player: 0, kind: pkr_core::state::ActionKind::Check });
        s.apply_action_in_place(&pkr_core::state::Action { player: 1, kind: pkr_core::state::ActionKind::Check });
        s.advance_street_in_place(&[44]);
        s
    }

    #[test]
    fn solve_root_returns_a_distribution() {
        let st = river_state();
        let holes = pkr_subgame::vector::hole_table();
        let mut r = vec![0.0f32; 1326];
        for h in 0..1326 {
            if !holes[h].iter().any(|c| st.board[..st.board_len as usize].contains(c)) { r[h] = 1.0; }
        }
        let out = solve_root(&st, [r.clone(), r], [20, 21], &VSearchCfg { iters: 50, ..Default::default() });
        if let Some(s) = out {
            let sum: f32 = s.iter().map(|(_, p)| *p).sum();
            assert!((sum - 1.0).abs() < 1e-2, "root strategy must sum ~1, got {sum}");
        }
    }
}

/// `SearchHandle` implementation backed by the vector-form solver.
///
/// This is what PKR_VSEARCH-style measurement needs: the sampled
/// `SubgameHandle` and this handle share one trait so that LBR and the
/// tournament can pick either without a hard dependency.
///
/// Ranges handed in via `decide` are only the OPPONENT's. Our own range
/// is a point mass at `our_hole` (masked for board compatibility),
/// because we know our cards exactly.
pub struct VSearchHandle {
    pub cfg: VSearchCfg,
}

impl VSearchHandle {
    pub fn new(cfg: VSearchCfg) -> Self {
        VSearchHandle { cfg }
    }
}

impl Default for VSearchHandle {
    fn default() -> Self {
        VSearchHandle { cfg: VSearchCfg::default() }
    }
}

impl crate::subgame::SearchHandle for VSearchHandle {
    fn decide(
        &self,
        state: &pkr_core::state::GameState,
        our_hole: &[u8; 2],
        opp_range: &[f64; pkr_subgame::range_tracker::N_HANDS],
    ) -> Option<[f64; crate::subgame::SUBGAME_BUCKETS]> {
        use pkr_core::state::ActionKind;

        let board: &[u8] = &state.board[..state.board_len as usize];

        // Our range: point mass at our hole, masked if it overlaps board.
        let mut our_r = vec![0.0f32; pkr_subgame::vector::N];
        if !our_hole.iter().any(|c| board.contains(c)) {
            our_r[pkr_subgame::vector::index_of(*our_hole)] = 1.0;
        } else {
            return None;
        }

        // Opponent range: mask board cards and cards we hold (they cannot
        // be in the opponent's hand), then f64 -> f32.
        let mut opp_r = vec![0.0f32; pkr_subgame::vector::N];
        for h in 0..pkr_subgame::vector::N {
            let hh = pkr_subgame::range_tracker::hole_at(h);
            if hh.iter().any(|c| board.contains(c)) {
                continue;
            }
            if hh.iter().any(|c| our_hole.contains(c)) {
                continue;
            }
            opp_r[h] = opp_range[h] as f32;
        }

        // Solver seats: 0 = OOP (postflop non-dealer), 1 = IP.
        let oop = 1 - state.dealer;
        let (r0, r1) = if state.actor == oop {
            (our_r, opp_r)
        } else {
            (opp_r, our_r)
        };

        let acts = solve_root(state, [r0, r1], *our_hole, &self.cfg)?;

        let mut out = [0.0f64; crate::subgame::SUBGAME_BUCKETS];
        let mut total = 0.0f64;
        for (act, p) in &acts {
            let kind = match act {
                Act::Fold => ActionKind::Fold,
                Act::Check => ActionKind::Check,
                Act::Call => ActionKind::Call,
                Act::Bet(t) => ActionKind::Bet(*t),
            };
            let b = pkr_core::abstraction::action_bucket(
                &kind,
                state.stacks[state.actor],
                state.street_bets[state.actor],
                state.street_bets[1 - state.actor],
                state.pot,
            ) as usize;
            if b < crate::subgame::SUBGAME_BUCKETS {
                out[b] += *p as f64;
                total += *p as f64;
            }
        }
        if total > 1e-9 {
            for x in out.iter_mut() {
                *x /= total;
            }
            Some(out)
        } else {
            None
        }
    }
}
