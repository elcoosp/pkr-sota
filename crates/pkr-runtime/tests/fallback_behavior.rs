//! Fallback behavior tests: verify SubgameHandle::decide returns None
//! in every condition where the blueprint path should be used instead.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::AbstractionBuilder;
use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_runtime::subgame::{SubgameConfig, SubgameHandle};
use pkr_subgame::range_tracker::N_HANDS;
use std::sync::Arc;

fn ws() -> std::path::PathBuf {
    let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    m.parent().unwrap().parent().unwrap().to_path_buf()
}
fn out(rel: &str) -> String {
    ws().join("outputs/v34long").join(rel).to_string_lossy().into_owned()
}

fn build_config() -> SubgameConfig {
    let store = load_centroids(&out("centroids.bin")).expect("centroids");
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, &out("preflop_abstraction.bin")).unwrap();
    abs.init_table(1, &out("abstraction.bin")).unwrap();
    abs.init_table(2, &out("turn_abstraction.bin")).unwrap();
    abs.init_table(3, &out("river_buckets.bin")).unwrap();
    let abs_arc: Arc<dyn AbstractionBuilder> = Arc::new(abs);

    let table = CompactRegretTable::with_capacity(60_000_000);
    let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(200);
    table.load_checkpoint(&out("train.ckpt"), &fp).expect("ckpt");

    SubgameConfig {
        evaluator: Arc::new(pkr_eval::NlheEvaluator),
        abstraction: abs_arc,
        table: Arc::new(table),
        iters: 10,
        hands_per_range: 6,
        enabled_streets: [false, false, true, true],
    }
}

fn uniform_opp_range() -> Box<[f64; N_HANDS]> {
    let mut r = Box::new([0.0f64; N_HANDS]);
    let n = N_HANDS as f64;
    for i in 0..N_HANDS { r[i] = 1.0 / n; }
    r
}

fn flop_p0_to_act() -> GameState {
    let b: [u8; 3] = [0, 14, 28];
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.advance_street_in_place(&b);
    s
}

fn preflop_p1_to_act() -> GameState {
    // Preflop: P0 (SB) calls, P1 (BB) to act.
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    s
}

#[test]
#[ignore]
fn fallback_returns_none_correctly() {
    let handle = SubgameHandle::new(build_config());
    let hole: [u8; 2] = [3, 5];
    let opp = uniform_opp_range();

    // ---- Test 1: disabled streets return None ----
    let st = GameState::new(200.0, 1.0, 2.0);
    println!("preflop: street={:?} actor={}", st.street, st.actor);
    assert_eq!(st.street, Street::Preflop);
    assert!(handle.decide(&st, &hole, &opp).is_none(),
        "preflop should return None (disabled)");

    let st = flop_p0_to_act();
    println!("flop:    street={:?} actor={}", st.street, st.actor);
    assert_eq!(st.street, Street::Flop);
    assert!(handle.decide(&st, &hole, &opp).is_none(),
        "flop should return None (disabled)");

    // ---- Test 2: actor guard ----
    // Enable all streets so we can isolate the actor condition.
    let mut cfg = build_config();
    cfg.enabled_streets = [true, true, true, true];
    let handle_all = SubgameHandle::new(cfg);

    let st = preflop_p1_to_act();
    println!("actor_guard: street={:?} actor={}", st.street, st.actor);
    assert_eq!(st.actor, 1, "setup should put P1 to act");
    assert!(handle_all.decide(&st, &hole, &opp).is_none(),
        "P1-to-act should return None even with street enabled");

    // ---- Test 3: street_enabled ----
    assert!(!handle.street_enabled(0), "preflop disabled");
    assert!(!handle.street_enabled(1), "flop disabled");
    assert!(handle.street_enabled(2), "turn enabled");
    assert!(handle.street_enabled(3), "river enabled");

    println!("all fallback checks passed");
}
