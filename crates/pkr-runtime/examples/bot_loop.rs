//! Minimal bot loop using RuntimeSession.
//!
//! Runnable skeleton — it drives a scripted deal against a fixed
//! opponent line, showing the session lifecycle end to end.
//!
//! Run:
//!   cargo run --release -p pkr-runtime --example bot_loop

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_runtime::session::RuntimeSession;
use pkr_runtime::subgame::{SubgameConfig, SubgameHandle};
use std::sync::Arc;

fn workspace() -> std::path::PathBuf {
    let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    m.parent().unwrap().parent().unwrap().to_path_buf()
}

fn table_path(rel: &str) -> String {
    workspace().join("outputs/v34long").join(rel).to_string_lossy().into_owned()
}

fn street_name(s: Street) -> &'static str {
    match s {
        Street::Preflop => "preflop",
        Street::Flop => "flop",
        Street::Turn => "turn",
        Street::River => "river",
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = load_centroids(&table_path("centroids.bin"))?;
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, &table_path("preflop_abstraction.bin"))?;
    abs.init_table(1, &table_path("abstraction.bin"))?;
    abs.init_table(2, &table_path("turn_abstraction.bin"))?;
    abs.init_table(3, &table_path("river_buckets.bin"))?;
    let abs_arc: Arc<dyn AbstractionBuilder> = Arc::new(abs);

    let table = CompactRegretTable::with_capacity(60_000_000);
    let fp = AbstractionFingerprint::from_constants(200);
    table.load_checkpoint(&table_path("train.ckpt"), &fp)?;
    let table = Arc::new(table);

    let handle = SubgameHandle::new(SubgameConfig {
        evaluator: Arc::new(pkr_eval::NlheEvaluator),
        abstraction: abs_arc.clone(),
        table: table.clone(),
        iters: 10,
        hands_per_range: 4,
        enabled_streets: [false, false, false, true],
    });

    let ev = pkr_eval::NlheEvaluator;
    let mut session = RuntimeSession::new(
        handle,
        0,
        abs_arc.as_ref(),
        table.as_ref(),
        &ev,
    );

    let mut root = GameState::new(200.0, 1.0, 2.0);
    root.set_hole_cards([0, 5], [40, 41]);
    session.deal_start(root.clone());

    // Legal line: preflop SB completes, BB checks. Then check-check
    // on every postflop street. Streets advance when the betting
    // round is complete (which is what `is_street_complete` reports).
    let runouts: &[&[u8]] = &[&[2, 6, 10], &[14], &[18]];
    let actions: &[(usize, ActionKind)] = &[
        (0, ActionKind::Call),
        (1, ActionKind::Check),
        (0, ActionKind::Check),
        (1, ActionKind::Check),
        (0, ActionKind::Check),
        (1, ActionKind::Check),
        (0, ActionKind::Check),
        (1, ActionKind::Check),
    ];

    let mut st = root.clone();
    let mut runout_idx = 0;
    for (player, kind) in actions {
        // Advance to the next street if the previous one is complete.
        while st.is_street_complete() && runout_idx < runouts.len() {
            let cards = runouts[runout_idx];
            runout_idx += 1;
            st.advance_street_in_place(cards);
            session.observe_street(cards);
            println!("[street] {} board_len={}", street_name(st.street), st.board_len);
        }
        let actor_before = st.actor;
        let a = Action { player: *player, kind: kind.clone() };
        st.apply_action_in_place(&a);
        session.observe_action(a);
        println!(
            "[action] actor={} player={} kind={:?}",
            actor_before, player, kind
        );
    }

    assert_eq!(st.street, Street::River);

    if st.actor == 0 {
        let mut sig_buf = [0u8; 8];
        let sig_len = st.infoset_signature_into(&mut sig_buf);
        let history = &sig_buf[..sig_len];
        let board = &st.board[..st.board_len as usize];
        let hash = abs_arc.get_infoset_hash(&st.hole[0], board, history, st.street as u8);

        let strat = session
            .advise_or_blueprint(&st, &st.hole[0], hash)
            .expect("advise_or_blueprint never returns None with a valid hash");

        println!();
        println!("=== Advice at river ===");
        for (i, p) in strat.iter().enumerate() {
            println!("  bucket {}: {:.4}", i, p);
        }
    } else {
        println!("not our turn (actor={})", st.actor);
    }

    Ok(())
}
