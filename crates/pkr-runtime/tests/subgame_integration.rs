//! End-to-end runtime integration test.

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

fn build_handle() -> SubgameHandle {
    let store = load_centroids(&out("centroids.bin")).expect("centroids");
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, &out("preflop_abstraction.bin")).unwrap();
    abs.init_table(1, &out("abstraction.bin")).unwrap();
    abs.init_table(2, &out("turn_abstraction.bin")).unwrap();
    abs.init_table(3, &out("river_buckets.bin")).unwrap();
    let abs_arc: Arc<dyn AbstractionBuilder> = Arc::new(abs);

    let table = CompactRegretTable::with_capacity(60_000_000);
    let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(200);
    table
        .load_checkpoint(&out("train.ckpt"), &fp)
        .expect("checkpoint");

    SubgameHandle::new(SubgameConfig {
        evaluator: Arc::new(pkr_eval::NlheEvaluator),
        abstraction: abs_arc,
        table: Arc::new(table),
        iters: 15,
        hands_per_range: 6,
        enabled_streets: [false, false, true, true],
    })
}

/// Turn root where BB (P1) has just checked, SB (P0) to act.
/// `state.actor == 0`, `!state.is_street_complete()`.
fn turn_root_p0_to_act() -> GameState {
    let b: [u8; 4] = [0, 14, 28, 42];
    let mut s = GameState::new(200.0, 1.0, 2.0);
    // Preflop: SB call, BB check
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.advance_street_in_place(&b[0..3]);
    // Flop: check, check
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.advance_street_in_place(&b[3..4]);
    // Turn: BB (P1) checks -> P0 (SB) to act
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s
}

fn uniform_opp_range() -> Box<[f64; N_HANDS]> {
    let mut r = Box::new([0.0f64; N_HANDS]);
    let n = N_HANDS as f64;
    for i in 0..N_HANDS {
        r[i] = 1.0 / n;
    }
    r
}

#[test]
#[ignore]
fn subgame_handle_decides_turn() {
    let handle = build_handle();
    let state = turn_root_p0_to_act();

    println!();
    println!("=== state setup ===");
    println!("  street:     {:?}", state.street);
    println!("  actor:      {}", state.actor);
    println!("  is_terminal: {}", state.is_terminal());
    println!("  is_street_complete: {}", state.is_street_complete());
    println!("  pot:        {:.2}", state.pot);
    println!("  board_len:  {}", state.board_len);

    assert_eq!(state.street, Street::Turn);
    assert_eq!(state.actor, 0, "expected P0 to act");
    assert!(!state.is_street_complete(), "P0 still has a decision");
    assert!(!state.is_terminal(), "not terminal");

    let our_hole: [u8; 2] = [3, 5];
    let opp_range = uniform_opp_range();

    let s = handle
        .decide(&state, &our_hole, &opp_range)
        .expect("turn solve should return a strategy");

    let sum: f64 = s.iter().sum();
    println!();
    println!("=== SubgameHandle::decide ===");
    println!("  sum:     {:.6}", sum);
    println!("  buckets: {:?}", s);
    assert!((sum - 1.0).abs() < 1e-6, "not normalized: {:.6}", sum);
}
