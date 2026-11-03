//! Session-level shadow regression tests from the 2026-11-03 re-audit.
//!
//! Runs in its own test binary because `PKR_SHADOW` is read through a
//! `OnceLock` (process-global): this file's process is dedicated to
//! shadow-on semantics. No trained artifacts are needed (stub
//! abstraction + empty regret table).

use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::AbstractionBuilder;
use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_runtime::session::RuntimeSession;
use pkr_runtime::subgame::{SubgameConfig, SubgameHandle};
use pkr_subgame::range_tracker::N_HANDS;

/// Deterministic FNV-1a infoset hash; with the empty regret table every
/// blueprint lookup is uniform.
struct StubAbs;

impl AbstractionBuilder for StubAbs {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in hole.iter().chain(board).chain(history) {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= street as u64;
        h
    }
}

/// `Action::player` is documentary; apply to the current actor.
fn act(st: &mut GameState, kind: ActionKind) {
    let p = st.actor;
    st.apply_action_in_place(&Action { player: p, kind });
}

/// Turn spot, dealer = 0 so seat 1 (OOP) acts first: seat 1 (our_seat)
/// is about to jam.
fn turn_our_seat_1_to_act() -> GameState {
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.set_hole_cards([0, 5], [10, 15]);
    act(&mut s, ActionKind::Call);
    act(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[20, 25, 30]);
    act(&mut s, ActionKind::Check);
    act(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[35]);
    assert_eq!(s.street, Street::Turn);
    assert_eq!(s.actor, 1, "fixture: seat 1 (OOP) acts first on the turn");
    s
}

/// Audit item 3 (session level): the opponent bets while the SHADOW
/// offers no bet action (they face our shadow all-in). The old fallback
/// applied a phantom FOLD: the shadow AND the tracker went terminal and
/// the rest of the hand's observations were dead. After the fix the
/// shadow falls back to Call, both states stay alive, and a subsequent
/// own action still advances them correctly.
#[test]
fn session_shadow_survives_opponent_bet_facing_our_all_in() {
    std::env::set_var("PKR_SHADOW", "1");

    let tbl = CompactRegretTable::with_capacity(1);
    let abs = StubAbs;
    let ev = pkr_eval::NlheEvaluator;
    let abs_ref: &dyn AbstractionBuilder = &abs;
    let ev_ref: &dyn pkr_contracts::Evaluator = &ev;
    let handle = SubgameHandle::new(SubgameConfig {
        evaluator: std::sync::Arc::new(pkr_eval::NlheEvaluator),
        abstraction: std::sync::Arc::new(StubAbs),
        table: std::sync::Arc::new(CompactRegretTable::with_capacity(1)),
        iters: 1,
        hands_per_range: 4,
        enabled_streets: [false, false, false, false],
    });
    let mut sess = RuntimeSession::new(handle, 1, abs_ref, &tbl, ev_ref);

    let root = turn_our_seat_1_to_act();
    sess.deal_start(root.clone());
    assert!(
        sess.shadow_state().is_some(),
        "PKR_SHADOW=1 must create the shadow"
    );

    // We (seat 1) jam: in-tree (facing no bet) for both real and shadow.
    sess.observe_action(Action {
        player: 1,
        kind: ActionKind::Bet(198.0),
    });
    let sh = sess.shadow_state().unwrap();
    assert_eq!(sh.street_bets[1], 198.0, "shadow must record our all-in");
    assert_eq!(sh.stacks[1], 0.0);
    let t = sess.tracker_state().unwrap();
    assert_eq!(t.street_bets[1], 198.0, "tracker must record our all-in");

    // The opponent (seat 0) bets off-tree although they face our all-in
    // (a divergent/hostile host; this is exactly the input that made the
    // old shadow apply a phantom Fold).
    sess.observe_action(Action {
        player: 0,
        kind: ActionKind::Bet(150.0),
    });

    let sh = sess.shadow_state().unwrap();
    assert!(
        !sh.is_terminal(),
        "shadow must NOT go terminal on the opponent bet (phantom fold)"
    );
    let last_shadow_kind = sh.history[sh.history_len as usize - 1].kind;
    assert!(
        !matches!(last_shadow_kind, ActionKind::Fold),
        "shadow must not record a fold for the opponent, got {last_shadow_kind:?}"
    );
    let t = sess.tracker_state().unwrap();
    assert!(
        !t.is_terminal(),
        "tracker must NOT go terminal on the opponent bet (phantom fold)"
    );
    assert_eq!(
        t.street_bets[0], 198.0,
        "opponent must have called the all-in in the tracker, not folded"
    );
    let opp = sess.opp_range().unwrap();
    let sum: f64 = opp.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-6,
        "posterior must stay normalized: {sum}"
    );

    // A subsequent own action still updates correctly: advance the
    // street (river, seat 1 = us to act) and check.
    sess.observe_street(&[44]);
    assert_eq!(sess.shadow_state().unwrap().street, Street::River);
    let actor_before = sess.tracker_state().unwrap().actor;
    assert_eq!(actor_before, 1, "river: seat 1 to act");
    sess.observe_action(Action {
        player: 1,
        kind: ActionKind::Check,
    });
    let sh = sess.shadow_state().unwrap();
    let t = sess.tracker_state().unwrap();
    assert_eq!(t.actor, 0, "our check must advance the tracker state");
    assert_eq!(sh.actor, 0, "our check must advance the shadow state");
    let opp = sess.opp_range().unwrap();
    let sum2: f64 = opp.iter().sum();
    assert!(
        (sum2 - 1.0).abs() < 1e-6,
        "posterior still normalized: {sum2}"
    );
    let _ = N_HANDS; // keep the import meaningful if assertions change
}
