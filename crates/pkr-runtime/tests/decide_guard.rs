//! Unit test for the all-zero opp_range guard in `SubgameHandle::decide`.
//!
//! The guard runs after the street-enabled gate so it does not slow the
//! disabled-street path. It rejects an all-zero posterior (which would
//! be a caller error) with `None` rather than producing a meaningless
//! strategy.

use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::AbstractionBuilder;
use pkr_core::state::GameState;
use pkr_runtime::subgame::{SubgameConfig, SubgameHandle};
use pkr_subgame::range_tracker::N_HANDS;
use std::sync::Arc;

fn build_handle(enabled_river: bool) -> SubgameHandle {
    let abs: Arc<dyn AbstractionBuilder> = Arc::new(
        pkr_abstraction::KMeansAbstraction::new(vec![], Arc::new(pkr_eval::NlheEvaluator)),
    );
    let cfg = SubgameConfig {
        evaluator: Arc::new(pkr_eval::NlheEvaluator),
        abstraction: abs,
        table: Arc::new(CompactRegretTable::with_capacity(1)),
        iters: 1,
        hands_per_range: 4,
        enabled_streets: [false, false, false, enabled_river],
    };
    SubgameHandle::new(cfg)
}

#[test]
fn all_zero_range_returns_none_on_enabled_street() {
    // Build a river state. We only need the street to be River and the
    // state to be non-terminal.
    //
    // IMPORTANT: the `player` field on Action must equal `state.actor`
    // at the moment of application. Preflop SB (P0) acts first;
    // postflop BB (P1) acts first in heads-up. The helper below uses
    // `st.actor` so we never have to think about the alternation.
    fn apply_here(st: &mut GameState, kind: pkr_core::state::ActionKind) {
        let actor = st.actor;
        st.apply_action_in_place(&pkr_core::state::Action {
            player: actor,
            kind,
        });
    }

    let mut st = GameState::new(200.0, 1.0, 2.0);
    use pkr_core::state::ActionKind;

    // Preflop: SB calls, BB checks.
    apply_here(&mut st, ActionKind::Call);
    apply_here(&mut st, ActionKind::Check);

    // Flop / turn: BB acts first (OOP), then SB. Both check.
    st.advance_street_in_place(&[0, 4, 8]);
    apply_here(&mut st, ActionKind::Check);
    apply_here(&mut st, ActionKind::Check);

    st.advance_street_in_place(&[12]);
    apply_here(&mut st, ActionKind::Check);
    apply_here(&mut st, ActionKind::Check);

    st.advance_street_in_place(&[16]);

    assert_eq!(st.street, pkr_core::state::Street::River);

    let handle = build_handle(true);
    let hole = [30u8, 31u8];
    let zero_range = [0.0f64; N_HANDS];

    let r = handle.decide(&st, &hole, &zero_range);
    assert!(r.is_none(), "all-zero range must return None");
}

#[test]
fn disabled_street_short_circuits_before_the_guard() {
    // Even a malformed (all-zero) range must not be read when the street
    // is disabled. This is a contract check: the guard sits below the
    // street gate, so `decide` returns None without touching opp_range.
    let st = GameState::new(200.0, 1.0, 2.0);
    let handle = build_handle(false);
    let hole = [30u8, 31u8];
    let zero_range = [0.0f64; N_HANDS];
    assert!(handle.decide(&st, &hole, &zero_range).is_none());
}
