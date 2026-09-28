//! Smoke test for RuntimeSession — verifies the stateful wrapper
//! correctly owns and updates the tracker.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_runtime::session::RuntimeSession;
use pkr_runtime::subgame::{SubgameConfig, SubgameHandle};
use std::sync::Arc;

fn ws() -> std::path::PathBuf {
    let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    m.parent().unwrap().parent().unwrap().to_path_buf()
}
fn out(rel: &str) -> String {
    ws().join("outputs/v34long").join(rel).to_string_lossy().into_owned()
}

fn build_handle() -> SubgameHandle {
    let store = load_centroids(&out("centroids.bin")).expect("centroids");
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, &out("preflop_abstraction.bin")).unwrap();
    abs.init_table(1, &out("abstraction.bin")).unwrap();
    abs.init_table(2, &out("turn_abstraction.bin")).unwrap();
    abs.init_table(3, &out("river_buckets.bin")).unwrap();
    let abs_arc: Arc<dyn AbstractionBuilder> = Arc::new(abs);

    let table = CompactRegretTable::with_capacity(60_000_000);
    let fp = AbstractionFingerprint::from_constants(200);
    table.load_checkpoint(&out("train.ckpt"), &fp).expect("ckpt");

    SubgameHandle::new(SubgameConfig {
        evaluator: Arc::new(pkr_eval::NlheEvaluator),
        abstraction: abs_arc,
        table: Arc::new(table),
        iters: 3,
        hands_per_range: 4,
        enabled_streets: [false, false, false, true],
    })
}

#[test]
#[ignore]
fn session_owns_tracker_and_updates() {
    let handle = build_handle();

    // Rebuild the same abs/table/evaluator as references for the session.
    let store = load_centroids(&out("centroids.bin")).expect("centroids");
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, &out("preflop_abstraction.bin")).unwrap();
    abs.init_table(1, &out("abstraction.bin")).unwrap();
    abs.init_table(2, &out("turn_abstraction.bin")).unwrap();
    abs.init_table(3, &out("river_buckets.bin")).unwrap();
    let abs_ref: &dyn AbstractionBuilder = &abs;

    let table = CompactRegretTable::with_capacity(60_000_000);
    let fp = AbstractionFingerprint::from_constants(200);
    table.load_checkpoint(&out("train.ckpt"), &fp).expect("ckpt");
    let tbl_ref = &table;

    let ev = pkr_eval::NlheEvaluator;
    let ev_ref: &dyn Evaluator = &ev;

    let mut session = RuntimeSession::new(handle, 0, abs_ref, tbl_ref, ev_ref);

    assert!(!session.is_active(), "no deal yet");

    // Start a deal.
    let root = GameState::new(200.0, 1.0, 2.0);
    session.deal_start(root);
    assert!(session.is_active(), "deal active");

    // Play a scripted preflop -> flop -> turn -> river line.
    session.observe_action(Action { player: 0, kind: ActionKind::Call });
    session.observe_action(Action { player: 1, kind: ActionKind::Check });
    session.observe_street(&[0, 4, 8]);
    session.observe_action(Action { player: 0, kind: ActionKind::Check });
    session.observe_action(Action { player: 1, kind: ActionKind::Check });
    session.observe_street(&[12]);
    session.observe_action(Action { player: 0, kind: ActionKind::Check });
    session.observe_action(Action { player: 1, kind: ActionKind::Check });
    session.observe_street(&[16]);

    // Build the same scripted state to query the session.
    let mut st = GameState::new(200.0, 1.0, 2.0);
    st.set_hole_cards([30, 31], [40, 41]);
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    st.advance_street_in_place(&[0, 4, 8]);
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    st.advance_street_in_place(&[12]);
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    st.advance_street_in_place(&[16]);

    // Sanity: this is a river decision. The actor may be 0 or 1
    // depending on whose turn it is post-river-advance.
    assert_eq!(st.street, Street::River);

    // Note: the session's internal tracker has a DIFFERENT root (no hole
    // cards set), but for a smoke test we only check the wrapper
    // mechanics — the tracker's posterior is exercised by tracker_probe.
    let opp_range = session.opp_range().expect("range available after deal_start");
    let sum: f64 = opp_range.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-6,
        "opp range must be normalized: sum = {sum}"
    );

    // Contract 1: advise() returns None when it's not our turn.
    if st.actor != 0 {
        let advice = session.advise(&st, &[30, 31]);
        assert!(
            advice.is_none(),
            "advise must return None when actor != our_seat (actor={})",
            st.actor
        );
    }

    // Contract 2: after the opponent acts, it's our turn and advise()
    // returns a normalized strategy.
    st.apply_action_in_place(&Action { player: st.actor, kind: ActionKind::Check });
    assert_eq!(st.actor, 0, "P0 should act after P1's check");
    let advice = session.advise(&st, &[30, 31]);
    assert!(advice.is_some(), "advise should return Some at river on our turn");
    let s = advice.unwrap();
    let ssum: f64 = s.iter().sum();
    assert!(
        (ssum - 1.0).abs() < 1e-6,
        "advice must be normalized: sum = {ssum}"
    );
}
