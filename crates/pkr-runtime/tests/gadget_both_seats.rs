//! End-to-end CFR-D gadget verification: both seats, real ranges,
//! real blueprint. `#[ignore]`d (needs artifacts); run with
//! `--ignored --nocapture`.

use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_cfr::table::CompactRegretTable;
use pkr_core::abstraction::AbstractionFingerprint;
use pkr_core::state::{Action, ActionKind, GameState, Street};
use std::sync::Arc;

fn ws() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR = <ws>/crates/pkr-runtime
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().parent().unwrap().to_path_buf()
}
fn tables_dir() -> std::path::PathBuf {
    std::env::var("PKR_TEST_TABLES").map(Into::into)
        .unwrap_or_else(|_| ws().join("outputs/v42-post-audit"))
}
fn ckpt() -> std::path::PathBuf {
    std::env::var("PKR_TEST_CKPT").map(Into::into)
        .unwrap_or_else(|_| ws().join("outputs/ab-v5/train.ckpt"))
}

fn load() -> Option<(Arc<KMeansAbstraction>, CompactRegretTable)> {
    // Set sig env BEFORE any fingerprint call.
    std::env::set_var("PKR_SIG_V5", "1");
    std::env::set_var("PKR_SIG_V4", "0");
    std::env::set_var("PKR_SIG_V3", "1");
    let t = tables_dir();
    let ck = ckpt();
    if !ck.exists() { eprintln!("SKIP: {} missing", ck.display()); return None; }
    let store = load_centroids(t.join("centroids.bin").to_str()?).ok()?;
    let abs = Arc::new(KMeansAbstraction::from_store(store, Arc::new(pkr_eval::NlheEvaluator)));
    abs.init_table(0, t.join("preflop_abstraction.bin").to_str()?).ok()?;
    abs.init_table(1, t.join("abstraction.bin").to_str()?).ok()?;
    abs.init_table(2, t.join("turn_abstraction.bin").to_str()?).ok()?;
    abs.init_table(3, t.join("river_buckets.bin").to_str()?).ok()?;
    let tbl = CompactRegretTable::with_capacity(60_000_000);
    let fp = AbstractionFingerprint::from_constants(abs.k() as u32);
    tbl.load_checkpoint(ck.to_str()?, &fp).ok()?;
    Some((abs, tbl))
}

/// River state. `ip_to_act=true` => seat 0 (IP, dealer=0) is to act.
fn river_state(ip_to_act: bool) -> GameState {
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.set_hole_cards([0, 5], [10, 15]);
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Call });
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.advance_street_in_place(&[20, 25, 30]);
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    s.advance_street_in_place(&[35]);
    s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    s.apply_action_in_place(&Action { player: 0, kind: ActionKind::Check });
    s.advance_street_in_place(&[44]);
    // river: actor = 1 (OOP) by default.
    if ip_to_act {
        s.apply_action_in_place(&Action { player: 1, kind: ActionKind::Check });
    }
    s
}

#[test]
#[ignore]
fn gadget_works_for_both_seats_with_real_blueprint() {
    let Some((abs, tbl)) = load() else { return };
    let abs_dyn: &dyn pkr_contracts::AbstractionBuilder = abs.as_ref();
    let holes = pkr_subgame::vector::hole_table();

    for ip in [false, true] {
        let st = river_state(ip);
        assert_eq!(st.street, Street::River, "fixture must be river");
        let oop = 1 - st.dealer;
        let actor_is_oop = st.actor == oop;
        assert_eq!(actor_is_oop, !ip, "fixture seat sanity (ip={ip})");

        let our = st.hole[st.actor];
        let board = &st.board[..st.board_len as usize];
        let mut our_r = vec![0.0f32; 1326];
        our_r[pkr_subgame::vector::index_of(our)] = 1.0;
        let mut opp_r = vec![0.0f32; 1326];
        let mut n = 0usize;
        for h in 0..1326 {
            let hh = holes[h];
            if hh.iter().any(|c| board.contains(c)) { continue; }
            if hh.iter().any(|c| our.contains(c)) { continue; }
            opp_r[h] = 1.0; n += 1;
        }
        let u = 1.0 / n as f32;
        for x in opp_r.iter_mut() { if *x > 0.0 { *x = u; } }
        let ranges = if actor_is_oop {
            [our_r.clone(), opp_r.clone()]
        } else {
            [opp_r.clone(), our_r.clone()]
        };

        let cfg = pkr_runtime::vsearch::VSearchCfg {
            bet_fracs: vec![0.5, 1.0],
            max_raises: 2,
            iters: 30,
            enabled_streets: [false, false, false, true],
            bound: false,
            gadget: true,
            bound_max_steps: 12,
            time_budget_ms: 0,
        };

        let out = pkr_runtime::vsearch::solve_root_gadgetted(
            &st, ranges, our, &cfg, abs_dyn, &tbl,
        );
        let dist = out.unwrap_or_else(|| panic!("ip={ip}: returned None"));
        let sum: f32 = dist.iter().map(|(_, p)| *p).sum();
        eprintln!(
            "ip_to_act={ip}: {} root actions, sum={sum:.4}, dist={:?}",
            dist.len(),
            dist.iter().map(|(a, p)| (format!("{a:?}"), format!("{p:.3}"))).collect::<Vec<_>>()
        );
        assert!((sum - 1.0).abs() < 5e-2, "ip={ip}: distribution sums to {sum}");
        assert!(!dist.is_empty(), "ip={ip}: empty strategy");
    }

    // cbv sanity: finite, not all zero.
    let st = river_state(false);
    let holes = pkr_subgame::vector::hole_table();
    let our = st.hole[st.actor];
    let board = &st.board[..st.board_len as usize];
    let mut our_r = vec![0.0f32; 1326];
    our_r[pkr_subgame::vector::index_of(our)] = 1.0;
    let mut opp_r = vec![0.0f32; 1326];
    for h in 0..1326 {
        let hh = holes[h];
        if hh.iter().any(|c| board.contains(c)) || hh.iter().any(|c| our.contains(c)) { continue; }
        opp_r[h] = 1.0;
    }
    let ev = pkr_eval::NlheEvaluator;
    let poc = pkr_subgame::POCConfig {
        root: st.clone(),
        p0_range: pkr_subgame::Range::weighted(
            vec![our], vec![1.0],
        ),
        p1_range: pkr_subgame::Range::weighted(
            (0..1326).filter(|&h| opp_r[h] > 0.0).map(|h| holes[h]).collect(),
            (0..1326).filter(|&h| opp_r[h] > 0.0).map(|_| 1.0f64).collect(),
        ),
        iterations: 1,
        evaluator: &ev,
        blueprint: Some((abs_dyn, &tbl)),
    };
    let cbv = pkr_subgame::p1_cbv_per_hand(&poc);
    let finite = cbv.iter().all(|x| x.is_finite());
    let nonzero = cbv.iter().filter(|x| x.abs() > 1e-6).count();
    let mn = cbv.iter().cloned().fold(f64::INFINITY, f64::min);
    let mx = cbv.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    eprintln!("cbv: finite={finite} nonzero={nonzero} min={mn:.3} max={mx:.3}");
    assert!(finite, "cbv must be finite");
    assert!(nonzero > 100, "cbv should be non-trivial; only {nonzero} nonzero");
}
