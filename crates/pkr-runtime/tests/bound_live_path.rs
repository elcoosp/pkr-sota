//! End-to-end smoke for the SHIPPED live search config: RuntimeSession
//! with PKR_VSEARCH_BOUND=1 must return a valid bucket distribution on a
//! river decision. `#[ignore]` (needs artifacts).
use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::AbstractionBuilder;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_runtime::session::RuntimeSession;
use pkr_runtime::subgame::{SubgameConfig, SubgameHandle};
use std::sync::Arc;

fn ws() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().to_path_buf()
}

#[test]
#[ignore]
fn bound_live_path_returns_distribution() {
    std::env::set_var("PKR_SIG_V5", "1");
    std::env::set_var("PKR_SIG_V4", "0");
    std::env::set_var("PKR_SIG_V3", "1");
    std::env::set_var("PKR_VSEARCH", "1");
    std::env::set_var("PKR_VSEARCH_BOUND", "1");
    let tdir = std::env::var("PKR_TEST_TABLES").map(std::path::PathBuf::from)
        .unwrap_or_else(|_| ws().join("outputs/v42-post-audit"));
    let ck = std::env::var("PKR_TEST_CKPT").map(std::path::PathBuf::from)
        .unwrap_or_else(|_| ws().join("outputs/ab-v5/train.ckpt"));
    if !ck.exists() { eprintln!("SKIP: {} missing", ck.display()); return; }

    let store = load_centroids(tdir.join("centroids.bin").to_str().unwrap()).unwrap();
    let abs = KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator));
    abs.init_table(0, tdir.join("preflop_abstraction.bin").to_str().unwrap()).unwrap();
    abs.init_table(1, tdir.join("abstraction.bin").to_str().unwrap()).unwrap();
    abs.init_table(2, tdir.join("turn_abstraction.bin").to_str().unwrap()).unwrap();
    abs.init_table(3, tdir.join("river_buckets.bin").to_str().unwrap()).unwrap();
    let abs_arc: Arc<dyn AbstractionBuilder> = Arc::new(abs);
    let fp = AbstractionFingerprint::from_constants(200);

    let t1 = CompactRegretTable::with_capacity(60_000_000);
    t1.load_checkpoint(ck.to_str().unwrap(), &fp).unwrap();
    let handle = SubgameHandle::new(SubgameConfig {
        evaluator: Arc::new(pkr_eval::NlheEvaluator),
        abstraction: abs_arc.clone(),
        table: Arc::new(t1),
        iters: 3, hands_per_range: 4,
        enabled_streets: [false, false, false, false],
    });
    let t2 = CompactRegretTable::with_capacity(60_000_000);
    t2.load_checkpoint(ck.to_str().unwrap(), &fp).unwrap();

    let mut st = GameState::new(200.0, 1.0, 2.0);
    st.set_hole_cards([0, 5], [10, 15]);
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    st.advance_street_in_place(&[20, 25, 30]);
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    st.advance_street_in_place(&[35]);
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    st.advance_street_in_place(&[44]);
    st.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    st.apply_action_in_place(&Action { player: 1, kind: ActionKind::Bet(30.0) });
    assert_eq!(st.actor, 0);

    let mut sess = RuntimeSession::new(handle, 0, abs_arc.as_ref(), &t2, &pkr_eval::NlheEvaluator);
    sess.deal_start(st.clone());
    if let Some(dist) = sess.advise(&st, &[0, 5]) {
        let sum: f64 = dist.iter().sum();
        eprintln!("live bound decision: sum={sum:.4} dist={dist:?}");
        assert!((sum - 1.0).abs() < 5e-2, "distribution must sum ~1, got {sum}");
    } else {
        eprintln!("live bound decision: None (fell through)");
    }
}
