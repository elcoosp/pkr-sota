//! Fallback behavior tests: the conditions under which
//! `SubgameHandle::decide` answers vs. returns `None` (so the caller
//! falls back to the blueprint path).
//!
//! 2026-11-03 re-audit (item 9): the previous version of this file was
//! `#[ignore]`d (it needed trained artifacts from `outputs/v34long`,
//! which are not in the repo) and asserted the STALE contract
//! "P1-to-act should return None even with street enabled". That became
//! wrong when `decide` gained `mirror_to_seat0`: a seat-1 actor is now
//! mirrored to seat 0 and solved like any other state, so EITHER seat
//! can act on an enabled street. The tests below are artifact-free
//! (empty-centroid abstraction + empty regret table, like
//! `decide_guard.rs`) and pin the CURRENT contract. The state-level
//! dealer-swap property itself is covered separately by
//! `mirror_dealer.rs`.

use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::AbstractionBuilder;
use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_runtime::subgame::{SubgameConfig, SubgameHandle};
use pkr_subgame::range_tracker::N_HANDS;
use std::sync::Arc;

/// Artifact-free config: the empty `KMeansAbstraction` hashes fine and
/// the empty `CompactRegretTable` makes every blueprint lookup uniform,
/// so the raw CFR solve path is what gets exercised.
fn build_config(enabled_streets: [bool; 4]) -> SubgameConfig {
    let abs: Arc<dyn AbstractionBuilder> = Arc::new(pkr_abstraction::KMeansAbstraction::new(
        vec![],
        Arc::new(pkr_eval::NlheEvaluator),
    ));
    SubgameConfig {
        evaluator: Arc::new(pkr_eval::NlheEvaluator),
        abstraction: abs,
        table: Arc::new(CompactRegretTable::with_capacity(1)),
        iters: 10,
        hands_per_range: 6,
        enabled_streets,
    }
}

fn uniform_opp_range() -> Box<[f64; N_HANDS]> {
    let mut r = Box::new([0.0f64; N_HANDS]);
    let n = N_HANDS as f64;
    for i in 0..N_HANDS {
        r[i] = 1.0 / n;
    }
    r
}

/// `Action::player` is documentary; apply to the current actor.
fn act(st: &mut GameState, kind: ActionKind) {
    let p = st.actor;
    st.apply_action_in_place(&Action { player: p, kind });
}

/// Check-check all the way to the river, nobody facing a bet. Seat 1
/// (OOP = BB) acts first on every postflop street, so the actor is 1.
fn river_p1_to_act() -> GameState {
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.set_hole_cards([3, 5], [10, 15]);
    act(&mut s, ActionKind::Call);
    act(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[20, 25, 30]);
    act(&mut s, ActionKind::Check);
    act(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[35]);
    act(&mut s, ActionKind::Check);
    act(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[44]);
    assert_eq!(s.street, Street::River);
    assert_eq!(s.actor, 1, "fixture: P1 (OOP/BB) to act on the river");
    s
}

/// Disabled streets must return None (the whole point of the gate).
#[test]
fn disabled_streets_return_none() {
    let handle = SubgameHandle::new(build_config([false, false, false, false]));
    let st = river_p1_to_act();
    let hole: [u8; 2] = [3, 5];
    assert!(
        handle.decide(&st, &hole, &uniform_opp_range()).is_none(),
        "every street disabled => None"
    );
    assert!(!handle.street_enabled(0) && !handle.street_enabled(1));
    assert!(!handle.street_enabled(2) && !handle.street_enabled(3));
}

/// CURRENT contract (item 9): with the street enabled, a P1-to-act
/// state is MIRRORED to seat 0 and solved — `decide` returns Some, not
/// the old "P1-to-act returns None" behaviour.
#[test]
fn p1_to_act_is_mirrored_and_solved_on_enabled_street() {
    let handle = SubgameHandle::new(build_config([false, false, true, true]));
    let st = river_p1_to_act();
    let hole: [u8; 2] = [3, 5];
    let out = handle
        .decide(&st, &hole, &uniform_opp_range())
        .expect("P1-to-act must be mirrored to seat 0 and solved");
    let sum: f64 = out.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-9,
        "root strategy must be a distribution, sum={sum}"
    );
    assert!(
        out.iter().all(|p| p.is_finite() && *p >= 0.0),
        "strategy must be finite and non-negative: {out:?}"
    );
}

/// Same contract for the seat-0 actor (unchanged by the mirror fix).
#[test]
fn p0_to_act_still_solves_on_enabled_street() {
    let handle = SubgameHandle::new(build_config([false, false, true, true]));
    let mut st = river_p1_to_act();
    // Seat 1 checks; now seat 0 acts on the river.
    act(&mut st, ActionKind::Check);
    assert_eq!(st.actor, 0);
    let hole: [u8; 2] = [10, 15];
    let out = handle.decide(&st, &hole, &uniform_opp_range());
    assert!(out.is_some(), "P0-to-act must still solve");
    let sum: f64 = out.unwrap().iter().sum();
    assert!((sum - 1.0).abs() < 1e-9, "sum={sum}");
}

/// Terminal states and an all-zero posterior must return None even on
/// an enabled street (caller falls back to the blueprint).
#[test]
fn terminal_state_and_zero_range_return_none() {
    let handle = SubgameHandle::new(build_config([false, false, true, true]));
    let hole: [u8; 2] = [3, 5];

    // Terminal: someone folds.
    let mut st = river_p1_to_act();
    act(&mut st, ActionKind::Fold);
    assert!(st.is_terminal());
    assert!(handle.decide(&st, &hole, &uniform_opp_range()).is_none());

    // All-zero posterior is a caller error; refuse it.
    let st = river_p1_to_act();
    let zero = [0.0f64; N_HANDS];
    assert!(
        handle.decide(&st, &hole, &zero).is_none(),
        "all-zero opp range must return None"
    );
}
