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

    // Legal line. Preflop: SB (P0) calls, BB (P1) checks.
    // Postflop: BB (P1) acts first (OOP in HU), then SB (P0).
    // Every action is applied with `player = st.actor` so the
    // debug_assert in `apply_action_internal` stays quiet.
    // Declared but unused: this example stops before P0 responds, so the
    // runout is never consumed. Kept to document the intended board.
    let _runouts: &[&[u8]] = &[&[2, 6, 10], &[14], &[18]];

    let mut st = root.clone();

    // Preflop.
    for kind in [ActionKind::Call, ActionKind::Check] {
        let actor = st.actor;
        let a = Action { player: actor, kind };
        st.apply_action_in_place(&a);
        session.observe_action(a);
        println!("[action] actor={} kind={:?}", actor, a.kind);
    }

    // Flop and turn: check-check. River: P1 (BB) bets, then P0 has a
    // decision — that is where the subgame path fires (river is in
    // `enabled_streets`, and it is our turn to act).
    //
    // Note: we only apply the P1 bet; P0's response is deliberately
    // NOT applied. `advise_or_blueprint` is called at that point, so
    // the state's actor is P0.
    let pre_river_runouts: &[&[u8]] = &[&[2, 6, 10], &[14]];
    let river_card: &[u8] = &[18];

    for cards in pre_river_runouts {
        if !st.is_street_complete() {
            eprintln!("BUG: expected street complete before advance");
            break;
        }
        st.advance_street_in_place(cards);
        session.observe_street(cards);
        println!("[street] {} board_len={}", street_name(st.street), st.board_len);
        for _ in 0..2 {
            let actor = st.actor;
            let a = Action { player: actor, kind: ActionKind::Check };
            st.apply_action_in_place(&a);
            session.observe_action(a);
            println!("[action] actor={} kind=Check", actor);
        }
    }

    // River: advance, P1 bets half-pot, P0 to act.
    st.advance_street_in_place(river_card);
    session.observe_street(river_card);
    println!("[street] {} board_len={}", street_name(st.street), st.board_len);

    let p1_bet = Action {
        player: 1,
        kind: ActionKind::Bet(st.pot * 0.5),
    };
    st.apply_action_in_place(&p1_bet);
    session.observe_action(p1_bet);
    println!(
        "[action] actor=1 kind=Bet({:.1}) pot_now={:.1}",
        st.pot * 0.5, st.pot
    );

    assert_eq!(st.street, Street::River);

    println!();
    if st.is_terminal() {
        println!("=== Hand over (river check-check → showdown) ===");
        println!("  folded: {:?}", st.folded);
        println!("  total_invested: {:?}", st.total_invested);
    } else if st.actor == 0 {
        let mut sig_buf = [0u8; 8];
        let sig_len = st.infoset_signature_into(&mut sig_buf);
        let history = &sig_buf[..sig_len];
        let board = &st.board[..st.board_len as usize];
        let hash = abs_arc.get_infoset_hash(&st.hole[0], board, history, st.street as u8);

        let strat = session
            .advise_or_blueprint(&st, &st.hole[0], hash)
            .expect("advise_or_blueprint never returns None with a valid hash");

        println!("=== Advice at river ===");
        for (i, p) in strat.iter().enumerate() {
            println!("  bucket {}: {:.4}", i, p);
        }
    } else {
        println!("not our turn (actor={})", st.actor);
    }

    Ok(())
}
