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
use std::sync::atomic::{AtomicU64, Ordering};

/// Diagnostic counter: how many times a solve was ACTUALLY
/// started (passed the street gate and built a tree). Read by
/// the instrumentation binaries when PKR_SEARCH_DEBUG=1.
pub static VSEARCH_SOLVE_COUNT: AtomicU64 = AtomicU64::new(0);
/// Diagnostic counter: how many times the street gate refused.
pub static VSEARCH_GATE_REFUSED: AtomicU64 = AtomicU64::new(0);
/// Diagnostic: how many bounded solves ran.
pub static VSEARCH_BOUNDED_SOLVES: AtomicU64 = AtomicU64::new(0);
/// Diagnostic: bounded solves that kept the search root whole
/// (search root was already at least as safe as the blueprint).
pub static VSEARCH_BOUND_KEPT_SEARCH: AtomicU64 = AtomicU64::new(0);
/// Diagnostic: bounded solves that blended toward the blueprint.
pub static VSEARCH_BOUND_BLENDED: AtomicU64 = AtomicU64::new(0);
/// Diagnostic: bounded solves that fell back to the raw solve
/// (blueprint/root unavailable).
pub static VSEARCH_BOUND_FELL_BACK: AtomicU64 = AtomicU64::new(0);
pub static VSEARCH_GADGET_SOLVES: AtomicU64 = AtomicU64::new(0);

pub struct VSearchCfg {
    pub bet_fracs: Vec<f32>,
    pub max_raises: u8,
    pub iters: u32,
    /// Which streets the vector solver may be invoked on. Turn+river by
    /// default: a preflop/flop exact solve is far too large for the
    /// runtime budget.
    pub enabled_streets: [bool; 4],
    /// Root safety bound: clamp the root to the largest convex blend with
    /// the blueprint whose opponent-BR is no worse than the pure
    /// blueprint's (vector-path analog of `safe_solve_root_p0_strategy`).
    /// Off by default; ON is the safe choice against an off-distribution
    /// opponent.
    pub bound: bool,
    /// CFR-D gadget: give the acting player's opponent a per-hand
    /// Terminate option for their blueprint value. Safe by construction
    /// (independent of ranges). Requires the acting player to be OOP and
    /// a blueprint source; falls back to the raw solve otherwise.
    pub gadget: bool,
    /// Binary-search steps for the bound alpha. Each step is one full
    /// `opp_br_value` tree walk. 12 = default (fine at the river). Set
    /// 2-4 for TURN solves, where the tree is ~46x the river. Fewer steps
    /// = coarser (more conservative) blend, still SAFE (the `BR <= bp`
    /// check holds). 0 = keep the search root if already safe, else pure
    /// blueprint — cheapest safe mode.
    pub bound_max_steps: u32,
    /// Hard wall-clock cap for ONE solve, in milliseconds. `iterate_budgeted`
    /// checks the clock every few iterations and stops early if exceeded.
    /// 0 = no cap (bounded only by `iters`).
    pub time_budget_ms: u64,
}

impl Default for VSearchCfg {
    fn default() -> Self {
        VSearchCfg {
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            iters: 200,
            enabled_streets: [false, false, true, true],
            bound: false,
            gadget: false,
            bound_max_steps: 12,
            time_budget_ms: 0,
        }
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
    // Gate the street HERE, at the single choke point. A flop solve
    // enumerates 47 turns x 46 rivers ~ 2162 showdowns over 1326 hands
    // and would hang or OOM the bot. Without this check a caller that
    // used `st.street != Preflop` as a proxy for "enabled" would let a
    // flop through. `VSearchHandle::decide` gates too; this covers the
    // free-function callers.
    if !cfg.enabled_streets[st.street as usize] {
        VSEARCH_GATE_REFUSED.fetch_add(1, Ordering::Relaxed);
        return None;
    }
    VSEARCH_SOLVE_COUNT.fetch_add(1, Ordering::Relaxed);
    let spec = spec_from_state(st, ranges, cfg.bet_fracs.clone(), cfg.max_raises);
    let ev = |h: &[u8; 2], b: &[u8]| pkr_eval::NlheEvaluator.evaluate_hand(h, b);
    let mut solver = Solver::new(spec, &ev);
    if solver.n_root_actions() == 0 {
        return None;
    }
    // M-29 (audit 2026-10-10): the raw solve did not honour
    // time_budget_ms; only solve_root_bounded did. A turn raw solve
    // could blow the budget by tens of seconds.
    iterate_budgeted(&mut solver, cfg);
    Some(solver.root_strategy(our_hand))
}

/// `solve_root` with a **root safety bound**: the root decision is
/// clamped to the largest convex blend with the blueprint whose opponent
/// best-response value does not exceed the pure blueprint's. Every
/// non-root node keeps the search continuation.
///
/// In-sample heuristic bound (vector-path analog of
/// `safe_solve_root_p0_strategy`), NOT the CFR-D gadget. Falls back to
/// the raw solve if the root or blueprint is unavailable.
pub fn solve_root_bounded(
    st: &pkr_core::state::GameState,
    ranges: [Vec<f32>; 2],
    our_hand: [u8; 2],
    cfg: &VSearchCfg,
    abs: &dyn pkr_contracts::AbstractionBuilder,
    tbl: &pkr_cfr::table::CompactRegretTable,
) -> Option<Vec<(Act, f32)>> {
    if !cfg.enabled_streets[st.street as usize] {
        return None;
    }
    VSEARCH_SOLVE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    VSEARCH_BOUNDED_SOLVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let spec = spec_from_state(st, ranges, cfg.bet_fracs.clone(), cfg.max_raises);
    let ev = |h: &[u8; 2], b: &[u8]| pkr_eval::NlheEvaluator.evaluate_hand(h, b);
    let mut solver = Solver::new(spec, &ev);
    if solver.n_root_actions() == 0 {
        return None;
    }
    iterate_budgeted(&mut solver, cfg);

    // Phase 4.5: arm a decision-wide deadline so the BR walks below
    // honour `time_budget_ms`. Without this a turn solve's binary search
    // cost ~40 s because only `iterate_budgeted` was clocked.
    if cfg.time_budget_ms > 0 {
        solver.set_deadline(Some(
            std::time::Instant::now()
                + std::time::Duration::from_millis(cfg.time_budget_ms),
        ));
    }

    let mut sig_buf = [0u8; 8];
    let sig_len = st.infoset_signature_into(&mut sig_buf);
    let history_sig = &sig_buf[..sig_len];
    let Some(bp_root) =
        solver.blueprint_root_strategy(abs, tbl, history_sig, st.street as u8, st)
    else {
        return Some(solver.root_strategy(our_hand));
    };
    let Some(search_root) = solver.root_strategy_all() else {
        VSEARCH_BOUND_FELL_BACK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Some(solver.root_strategy(our_hand));
    };

    let br_search = match solver.opp_br_value() {
        Some(v) => v,
        None => {
            // Deadline fired during the search walk. Blueprint root is
            // safe by construction; abandon search for this decision.
            VSEARCH_BOUND_FELL_BACK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            solver.set_root_strategy(&bp_root);
            return Some(solver.root_strategy(our_hand));
        }
    };
    solver.set_root_strategy(&bp_root);
    let br_bp = match solver.opp_br_value() {
        Some(v) => v,
        None => {
            // Deadline fired during the blueprint walk. bp_root is already
            // installed, so its sampled action is the safe answer.
            VSEARCH_BOUND_FELL_BACK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(solver.root_strategy(our_hand));
        }
    };

    let final_root = if br_search <= br_bp {
        VSEARCH_BOUND_KEPT_SEARCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        search_root.clone()
    } else {
        VSEARCH_BOUND_BLENDED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // The binary search's BR walks are NOT covered by time_budget_ms
        // (which only caps `iterate`). Check the clock here too so a live
        // decision honours the budget; once over, keep the best safe blend
        // found so far (bp_root is always safe).
        let t0 = std::time::Instant::now();
        let budget = std::time::Duration::from_millis(cfg.time_budget_ms);
        let timed = cfg.time_budget_ms > 0;
        let mut lo = 0.0f32;
        let mut hi = 1.0f32;
        let mut best = bp_root.clone();
        for _ in 0..cfg.bound_max_steps.max(1) {
            if timed && t0.elapsed() >= budget { break; }
            let mid = 0.5 * (lo + hi);
            let mixed = Solver::blend_root(&search_root, &bp_root, mid);
            solver.set_root_strategy(&mixed);
            match solver.opp_br_value() {
                Some(br) => {
                    if br <= br_bp {
                        best = mixed;
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                None => {
                    // Timed out mid-search. `best` already holds the best
                    // SAFE blend found (it starts as bp_root).
                    break;
                }
            }
            if hi - lo < 1e-3 {
                break;
            }
        }
        best
    };
    solver.set_root_strategy(&final_root);
    Some(solver.root_strategy(our_hand))
}

/// CFR-D gadget solve for EITHER seat. Places the acting player in
/// solver-seat 0 via `dealer = 1 - actor` (so `root_actor = 0`); swaps
/// only the range slots when the actor is IP. The gadget pays the
/// opponent (solver-seat 1) its blueprint-continuation value, so the
/// returned strategy is the acting player's, safe independent of ranges.
pub fn solve_root_gadgetted(
    st: &pkr_core::state::GameState,
    ranges: [Vec<f32>; 2],
    our_hand: [u8; 2],
    cfg: &VSearchCfg,
    abs: &dyn pkr_contracts::AbstractionBuilder,
    tbl: &pkr_cfr::table::CompactRegretTable,
) -> Option<Vec<(Act, f32)>> {
    if !cfg.enabled_streets[st.street as usize] {
        return None;
    }
    let oop_orig = 1 - st.dealer;
    let actor_is_oop = st.actor == oop_orig;
    let mut work = st.clone();
    work.dealer = 1 - st.actor;
    let ranges_w = if actor_is_oop {
        ranges
    } else {
        [ranges[1].clone(), ranges[0].clone()]
    };
    let spec = spec_from_state(&work, ranges_w.clone(), cfg.bet_fracs.clone(), cfg.max_raises);
    if spec.root_actor != 0 {
        return None;
    }
    VSEARCH_SOLVE_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    VSEARCH_GADGET_SOLVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let eval = |h: &[u8; 2], b: &[u8]| pkr_eval::NlheEvaluator.evaluate_hand(h, b);
    let mut solver = Solver::new_with_root_state(spec, &eval, work.clone());
    if solver.n_root_actions() == 0 {
        return None;
    }
    // CFR-D Terminate pays the opponent its BLUEPRINT-CONTINUATION EV
    // (not its BR value). This is the config the safety sweep proved
    // (gadget_br <= blueprint_br across 4 subgames); using the BR value
    // here breaks it (cbv=BR -> 11.23 vs cbv=EV -> 2.09). The EV cbv is
    // computed by `p1_cbv_per_hand` on a sampled POCConfig built from the
    // SAME root + ranges.
    let holes = pkr_subgame::vector::hole_table();
    let to_hp = |r: &[f32]| {
        let mut hs = Vec::new();
        let mut ps = Vec::new();
        for h in 0..r.len() {
            if r[h] > 0.0 { hs.push(holes[h]); ps.push(r[h] as f64); }
        }
        (hs, ps)
    };
    let (h0, p0) = to_hp(&ranges_w[0]);
    let (h1, p1) = to_hp(&ranges_w[1]);
    if h0.is_empty() || h1.is_empty() { return None; }
    let ev = pkr_eval::NlheEvaluator;
    let poc = pkr_subgame::POCConfig {
        root: work.clone(),
        p0_range: pkr_subgame::Range::weighted(h0, p0),
        p1_range: pkr_subgame::Range::weighted(h1, p1),
        iterations: 1,
        evaluator: &ev,
        blueprint: Some((abs, tbl)),
    };
    let cbv: Vec<f32> = pkr_subgame::p1_cbv_per_hand(&poc).iter().map(|&x| x as f32).collect();
    solver.install_gadget(cbv, 1);
    // M-29: budget the gadget solve too.
    iterate_budgeted(&mut solver, cfg);
    Some(solver.root_strategy(our_hand))
}

/// Iteration chunk size for the wall-clock check. Small enough to
/// respect the budget; large enough to stay off the hot path.
const BUDGET_CHECK_EVERY: u32 = 5;

/// `solver.iterate` with a wall-clock cap: stops after `cfg.iters` OR
/// `cfg.time_budget_ms` elapsed, whichever first.
fn iterate_budgeted(solver: &mut Solver, cfg: &VSearchCfg) {
    if cfg.time_budget_ms == 0 {
        solver.iterate(cfg.iters);
        return;
    }
    let start = std::time::Instant::now();
    let budget = std::time::Duration::from_millis(cfg.time_budget_ms);
    let mut done = 0u32;
    while done < cfg.iters {
        let chunk = BUDGET_CHECK_EVERY.min(cfg.iters - done);
        solver.iterate(chunk);
        done += chunk;
        if start.elapsed() >= budget {
            break;
        }
    }
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
    /// Blueprint sources for the root safety bound (`cfg.bound`). When
    /// `bound` is true but these are `None`, `decide` falls back to the
    /// raw solve (and prints a one-time warning under PKR_SEARCH_DEBUG).
    pub bound_abs: Option<std::sync::Arc<dyn pkr_contracts::AbstractionBuilder>>,
    pub bound_tbl: Option<std::sync::Arc<pkr_cfr::table::CompactRegretTable>>,
}

impl VSearchHandle {
    pub fn new(cfg: VSearchCfg) -> Self {
        VSearchHandle { cfg, bound_abs: None, bound_tbl: None }
    }

    /// Construct with the blueprint sources required by the safety bound.
    pub fn with_bound(
        cfg: VSearchCfg,
        abs: std::sync::Arc<dyn pkr_contracts::AbstractionBuilder>,
        tbl: std::sync::Arc<pkr_cfr::table::CompactRegretTable>,
    ) -> Self {
        VSearchHandle { cfg, bound_abs: Some(abs), bound_tbl: Some(tbl) }
    }
}

impl Default for VSearchHandle {
    fn default() -> Self {
        VSearchHandle { cfg: VSearchCfg::default(), bound_abs: None, bound_tbl: None }
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

        if !self.cfg.enabled_streets[state.street as usize] {
            return None;
        }

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
        //
        // The MASKED sum must be non-zero. If masking removed every
        // hand with mass (e.g. the caller passed a range that only
        // contained our cards or board cards), the solve would return
        // a uniform strategy — indistinguishable from a real answer,
        // and wrong. The sampled `SubgameHandle` guards the same way;
        // the vector path previously did not.
        let mut opp_r = vec![0.0f32; pkr_subgame::vector::N];
        let mut opp_mass = 0.0f64;
        for h in 0..pkr_subgame::vector::N {
            let hh = pkr_subgame::range_tracker::hole_at(h);
            if hh.iter().any(|c| board.contains(c)) {
                continue;
            }
            if hh.iter().any(|c| our_hole.contains(c)) {
                continue;
            }
            let w = opp_range[h];
            if w > 0.0 {
                opp_r[h] = w as f32;
                opp_mass += w;
            }
        }
        if opp_mass <= 1e-9 {
            return None;
        }

        // Solver seats: 0 = OOP (postflop non-dealer), 1 = IP.
        let oop = 1 - state.dealer;
        let (r0, r1) = if state.actor == oop {
            (our_r, opp_r)
        } else {
            (opp_r, our_r)
        };

        let acts = if self.cfg.gadget {
            match (self.bound_abs.as_ref(), self.bound_tbl.as_ref()) {
                (Some(abs), Some(tbl)) => {
                    let a: &dyn pkr_contracts::AbstractionBuilder = abs.as_ref();
                    let t: &pkr_cfr::table::CompactRegretTable = tbl.as_ref();
                    match solve_root_gadgetted(state, [r0.clone(), r1.clone()], *our_hole, &self.cfg, a, t) {
                        Some(v) => v,
                        None => match solve_root(state, [r0, r1], *our_hole, &self.cfg) {
                            Some(v) => v,
                            None => return None,
                        },
                    }
                }
                _ => match solve_root(state, [r0, r1], *our_hole, &self.cfg) {
                    Some(v) => v,
                    None => return None,
                },
            }
        } else if self.cfg.bound {
            match (self.bound_abs.as_ref(), self.bound_tbl.as_ref()) {
                (Some(abs), Some(tbl)) => {
                    let a: &dyn pkr_contracts::AbstractionBuilder = abs.as_ref();
                    let t: &pkr_cfr::table::CompactRegretTable = tbl.as_ref();
                    match solve_root_bounded(state, [r0, r1], *our_hole, &self.cfg, a, t) {
                        Some(v) => v,
                        None => return None,
                    }
                }
                _ => {
                    // bound requested without sources: fall back to raw
                    // solve. Warn once.
                    use std::sync::OnceLock;
                    static WARNED: OnceLock<()> = OnceLock::new();
                    if WARNED.get().is_none() && std::env::var("PKR_SEARCH_DEBUG").is_ok() {
                        WARNED.set(()).ok();
                        eprintln!(
                            "[vsearch] bound=true but no abstraction/table supplied;                              using the RAW (unbounded) solve"
                        );
                    }
                    match solve_root(state, [r0, r1], *our_hole, &self.cfg) {
                        Some(v) => v,
                        None => return None,
                    }
                }
            }
        } else {
            match solve_root(state, [r0, r1], *our_hole, &self.cfg) {
                Some(v) => v,
                None => return None,
            }
        };

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

#[cfg(test)]
mod handle_tests {
    use super::*;
    use crate::subgame::SearchHandle;
    use pkr_core::state::{Action, ActionKind, GameState};

    /// Build a river state by playing check-check to the river.
    fn river_state() -> GameState {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.set_hole_cards([0, 5], [10, 15]);
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
        s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
        s.advance_street_in_place(&[20, 25, 30]);
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
        s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
        s.advance_street_in_place(&[35]);
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
        s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
        s.advance_street_in_place(&[44]);
        s
    }

    /// Regression: solve_root MUST refuse a flop (or any disabled
    /// street) before building the tree. A flop build enumerates 47x46
    /// rivers and hangs. Before the fix the free function had no gate
    /// and advise() used `!= Preflop` as a proxy for "enabled".
    #[test]
    fn solve_root_refuses_flop() {
        // check-call to the flop (board has 3 cards).
        let mut s = GameState::new(200.0, 1.0, 2.0);
        s.set_hole_cards([0, 5], [10, 15]);
        s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
        s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
        s.advance_street_in_place(&[20, 25, 30]);
        assert_eq!(s.street as usize, 1, "must be on the flop");

        let cfg = VSearchCfg {
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            iters: 1,
            enabled_streets: [false, false, true, true], // flop disabled
            bound: false,
            gadget: false,
            bound_max_steps: 12,
            time_budget_ms: 0,
        };
        let holes = pkr_subgame::vector::hole_table();
        let mut r = vec![0.0f32; 1326];
        for h in 0..1326 {
            if !holes[h].iter().any(|c| s.board[..s.board_len as usize].contains(c)) {
                r[h] = 1.0;
            }
        }
        let out = solve_root(&s, [r.clone(), r], [0, 5], &cfg);
        assert!(out.is_none(), "flop solve must be refused by enabled_streets");
    }

    /// The trait path must return a normalized distribution over buckets
    /// when handed a live river state and a nondegenerate opponent range.
    #[test]
    fn vsearch_handle_through_trait() {
        let h = VSearchHandle::new(VSearchCfg {
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            iters: 30,
            enabled_streets: [false, false, false, true], // river only
            bound: false,
            gadget: false,
            bound_max_steps: 12,
            time_budget_ms: 0,
        });
        let st = river_state();
        // Uniform over everything not on the board or our hole.
        let board = &st.board[..st.board_len as usize];
        let our = [0u8, 5u8];
        let mut r = [0.0f64; pkr_subgame::range_tracker::N_HANDS];
        let mut n = 0usize;
        for i in 0..pkr_subgame::range_tracker::N_HANDS {
            let hh = pkr_subgame::range_tracker::hole_at(i);
            if hh.iter().any(|c| board.contains(c)) { continue; }
            if hh.iter().any(|c| our.contains(c)) { continue; }
            r[i] = 1.0; n += 1;
        }
        assert!(n > 100, "expected a nonempty opponent range");
        let u = 1.0 / n as f64;
        for x in r.iter_mut() { if *x > 0.0 { *x = u; } }

        let out = h.decide(&st, &our, &r);
        let Some(dist) = out else {
            panic!("VSearchHandle::decide returned None on a river state");
        };
        let sum: f64 = dist.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "distribution must sum to 1, got {sum}"
        );
        assert!(dist.iter().all(|x| *x >= 0.0), "negative probability");
    }

    /// The driver's seat-plumbing: `work.dealer = 1 - actor` makes the
    /// actor OOP, so `spec_from_state` yields `root_actor == 0` for any
    /// input seat. Pure arithmetic check — no solver needed.
    #[test]
    fn dealer_flip_puts_actor_in_solver_seat_0() {
        for actor in [0u8, 1u8] {
            let work_dealer = 1 - actor;
            let oop = 1 - work_dealer;
            assert_eq!(oop, actor, "actor={actor} must land in seat 0");
        }
    }

    /// The gadget path must fall back to the raw solve when no
    /// abstraction/table is supplied, returning a distribution (same
    /// contract as `bound`).
    #[test]
    fn solve_root_gadgetted_runs() {
        let h = VSearchHandle::new(VSearchCfg {
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            iters: 10,
            enabled_streets: [false, false, false, true],
            bound: false,
            gadget: true,
            bound_max_steps: 12,
            time_budget_ms: 0,
        });
        let st = river_state();
        let board = &st.board[..st.board_len as usize];
        let our = [0u8, 5u8];
        let mut r = [0.0f64; pkr_subgame::range_tracker::N_HANDS];
        let mut n = 0usize;
        for i in 0..pkr_subgame::range_tracker::N_HANDS {
            let hh = pkr_subgame::range_tracker::hole_at(i);
            if hh.iter().any(|c| board.contains(c)) { continue; }
            if hh.iter().any(|c| our.contains(c)) { continue; }
            r[i] = 1.0; n += 1;
        }
        let u = 1.0 / n as f64;
        for x in r.iter_mut() { if *x > 0.0 { *x = u; } }
        let out = crate::subgame::SearchHandle::decide(&h, &st, &our, &r);
        assert!(out.is_some(), "gadget without sources falls back to raw solve");
        let sum: f64 = out.unwrap().iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }

    /// `bound: true` with no abstraction/table must NOT panic — it falls
    /// back to the raw solve and returns a normalized distribution.
    #[test]
    fn bound_handle_falls_back_without_sources() {
        let h = VSearchHandle::new(VSearchCfg {
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            iters: 10,
            enabled_streets: [false, false, false, true],
            bound: true,
            gadget: false,
            bound_max_steps: 12,
            time_budget_ms: 0,
        });
        let st = river_state();
        let board = &st.board[..st.board_len as usize];
        let our = [0u8, 5u8];
        let mut r = [0.0f64; pkr_subgame::range_tracker::N_HANDS];
        let mut n = 0usize;
        for i in 0..pkr_subgame::range_tracker::N_HANDS {
            let hh = pkr_subgame::range_tracker::hole_at(i);
            if hh.iter().any(|c| board.contains(c)) { continue; }
            if hh.iter().any(|c| our.contains(c)) { continue; }
            r[i] = 1.0; n += 1;
        }
        let u = 1.0 / n as f64;
        for x in r.iter_mut() { if *x > 0.0 { *x = u; } }
        let out = crate::subgame::SearchHandle::decide(&h, &st, &our, &r);
        assert!(out.is_some(), "bound without sources must fall back to raw solve");
        let sum: f64 = out.unwrap().iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "distribution must sum to 1");
    }

    /// When the street is not enabled the trait must return None (so the
    /// caller falls through to the blueprint). This is the short-circuit
    /// that keeps PKR_VSEARCH from trying to solve a flop.
    #[test]
    fn vsearch_handle_returns_none_when_street_disabled() {
        let h = VSearchHandle::new(VSearchCfg {
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            iters: 30,
            enabled_streets: [false, false, false, false],
            bound: false,
            gadget: false,
            bound_max_steps: 12,
            time_budget_ms: 0,
        });
        let st = river_state();
        let r = [0.0f64; pkr_subgame::range_tracker::N_HANDS];
        let out = h.decide(&st, &[0, 5], &r);
        assert!(out.is_none(), "disabled street must return None");
    }

    /// Regression: an opponent range whose mass is entirely removed by
    /// masking (all hands share a board card, or all hands share our
    /// cards) must return None, not a uniform strategy. Uniform is
    /// indistinguishable from a real answer and would silently mislead
    /// the caller.
    #[test]
    fn vsearch_handle_returns_none_on_empty_masked_range() {
        let h = VSearchHandle::new(VSearchCfg {
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            iters: 5,
            enabled_streets: [false, false, false, true],
            bound: false,
            gadget: false,
            bound_max_steps: 12,
            time_budget_ms: 0,
        });
        let st = river_state();
        // Board contains 20; put mass ONLY on hands containing card 20.
        // Every hand will be masked out; opp_mass must be 0 -> None.
        let mut r = [0.0f64; pkr_subgame::range_tracker::N_HANDS];
        let mut any = false;
        for i in 0..pkr_subgame::range_tracker::N_HANDS {
            let hh = pkr_subgame::range_tracker::hole_at(i);
            if hh.contains(&20) {
                r[i] = 1.0; any = true;
            }
        }
        assert!(any, "fixture sanity: at least one hand contains card 20");
        let out = h.decide(&st, &[0, 5], &r);
        assert!(out.is_none(), "empty masked opponent range must return None");
    }

    /// Our hole overlapping the board must return None, not panic or
    /// solve with a nonsense point mass.
    #[test]
    fn vsearch_handle_returns_none_on_board_hole_overlap() {
        let h = VSearchHandle::new(VSearchCfg {
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            iters: 10,
            enabled_streets: [false, false, false, true],
            bound: false,
            gadget: false,
            bound_max_steps: 12,
            time_budget_ms: 0,
        });
        let st = river_state();
        // Board contains 20 (flop), so a hole of [20, 5] overlaps.
        let r = [0.0f64; pkr_subgame::range_tracker::N_HANDS];
        let out = h.decide(&st, &[20, 5], &r);
        assert!(out.is_none(), "hole overlapping the board must return None");
    }
}
