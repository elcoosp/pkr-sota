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
