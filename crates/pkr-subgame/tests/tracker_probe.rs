//! Probe: verify that RangeTracker's posterior at a river decision is
//! measurably non-uniform given a real blueprint. This is the essential
//! prerequisite for range-aware subgame solving; without it, the hook
//! would receive a uniform range and the whole design collapses.
//!
//! Uses the v34long checkpoint from outputs/ as the blueprint.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_subgame::range_tracker::RangeTracker;
use std::sync::Arc;

fn ws() -> std::path::PathBuf {
    let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    m.parent().unwrap().parent().unwrap().to_path_buf()
}
fn out(rel: &str) -> String {
    ws().join("outputs/v34long").join(rel).to_string_lossy().into_owned()
}

#[test]
#[ignore]
fn tracker_is_nonuniform_at_river() {
    let store = load_centroids(&out("centroids.bin")).expect("centroids");
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, &out("preflop_abstraction.bin")).unwrap();
    abs.init_table(1, &out("abstraction.bin")).unwrap();
    abs.init_table(2, &out("turn_abstraction.bin")).unwrap();
    abs.init_table(3, &out("river_buckets.bin")).unwrap();

    let table = CompactRegretTable::with_capacity(60_000_000);
    let fp = AbstractionFingerprint::from_constants(200);
    table.load_checkpoint(&out("train.ckpt"), &fp).expect("ckpt");

    let ev = pkr_eval::NlheEvaluator;
    let root = GameState::new(200.0, 1.0, 2.0);
    let mut tracker = RangeTracker::new(root, &abs, &table, &ev);

    // Scripted line: SB limp/call preflop -> check/bet/raise on flop -> etc.
    // We just need to reach a river state with a non-trivial history; the
    // exact line is unimportant, we only care that the posterior shifts.
    let line: &[(usize, ActionKind)] = &[
        (0, ActionKind::Call),
        (1, ActionKind::Check),
        // Flop
        (0, ActionKind::Check),
        (1, ActionKind::Bet(4.0)),
        (0, ActionKind::Call),
        // Turn
        (0, ActionKind::Check),
        (1, ActionKind::Bet(8.0)),
        (0, ActionKind::Call),
        // River
        (0, ActionKind::Check),
        (1, ActionKind::Bet(16.0)),
    ];

    // Approximate board runouts (real board doesn't matter for the probe;
    // the abstraction hashes on the board but any valid board exercises
    // the code path).
    let boards: [[u8; 3]; 3] = [[0, 4, 8], [12, 16, 20], [24, 28, 32]];
    let board_idx = 0usize;
    let mut pending_street_advance = false;
    let mut checked_river = false;

    for (i, (_actor, kind)) in line.iter().enumerate() {
        let action = Action { player: 0, kind: *kind };
        let _ = tracker.apply_action(action);

        // Advance the street after each completed street, using a fixed
        // board for each. Detect completion by counting actions taken.
        // Simpler: advance after specific indices.
        let _ = i;
        if let Some(&cards) = boards.get(board_idx) {
            // Advance when we know a betting round is complete. Use
            // state().is_street_complete() rather than counting.
            let _ = cards;
        }
        if tracker.state().is_street_complete()
            && !matches!(tracker.state().street, pkr_core::state::Street::River)
            && !pending_street_advance
        {
            // Pick the next 3-card board from the pool for flop, then 1
            // card on turn, 1 on river.
            let cards: Vec<u8> = match tracker.state().street {
                pkr_core::state::Street::Preflop => vec![0, 4, 8],
                pkr_core::state::Street::Flop => vec![12],
                pkr_core::state::Street::Turn => vec![16],
                _ => vec![],
            };
            if !cards.is_empty() {
                let _ = tracker.advance_street(&cards);
            }
            pending_street_advance = true;
        } else {
            pending_street_advance = false;
        }

        if matches!(tracker.state().street, pkr_core::state::Street::River)
            && !checked_river
        {
            checked_river = true;
        }
    }

    // Now inspect the posterior.
    let p0 = tracker.range(0);
    let p1 = tracker.range(1);
    let uniform = 1.0 / p0.len() as f64;

    let var_p0: f64 = p0.iter().map(|&x| (x - uniform).powi(2)).sum::<f64>() / p0.len() as f64;
    let var_p1: f64 = p1.iter().map(|&x| (x - uniform).powi(2)).sum::<f64>() / p1.len() as f64;

    let max_p0 = p0.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let max_p1 = p1.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    println!("uniform={:.6e}", uniform);
    println!("var_p0={:.6e} var_p1={:.6e}", var_p0, var_p1);
    println!("max_p0={:.6e} max_p1={:.6e}", max_p0, max_p1);
    println!("checked_river={}", checked_river);

    // If the tracker were uniform, var == 0 and max == uniform. A
    // non-trivial posterior has var > 0 and max >> uniform.
    assert!(checked_river, "probe never reached the river");
    assert!(
        var_p0 > uniform * 1e-6 || var_p1 > uniform * 1e-6,
        "tracker is uniform at river — nothing to build on"
    );
    assert!(
        max_p0 > uniform * 5.0 || max_p1 > uniform * 5.0,
        "no hand has mass >5x uniform — posterior is degenerate"
    );
}
