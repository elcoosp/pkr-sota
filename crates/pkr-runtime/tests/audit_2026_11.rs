//! Regression tests from the 2026-11-03 re-audit (Task 9).
//!
//! Each test targets one confirmed audit finding. Artifact-free by
//! design: the blueprint source is a stub `AbstractionBuilder` plus an
//! empty `CompactRegretTable` (every lookup returns the uniform
//! strategy), so no trained checkpoint is needed.

use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::AbstractionBuilder;
use pkr_core::state::{Action, ActionKind, GameState, Street};
use pkr_runtime::vsearch::{self, VSearchCfg};

/// Deterministic FNV-1a infoset hash. The VALUES do not matter for the
/// tests in this file; only that the trait is implemented so the solve
/// paths that consult a blueprint can run. Combined with an empty
/// `CompactRegretTable` every blueprint lookup is uniform.
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

/// `Action::player` is documentary (apply_action_internal applies to
/// `state.actor`), so every scripted action goes through this helper.
fn act(st: &mut GameState, kind: ActionKind) {
    let p = st.actor;
    st.apply_action_in_place(&Action { player: p, kind });
}

/// River spot where the actor FACES a raise after having bet this
/// street: `street_bets[actor] = 30 > 0`. That is the shape where the
/// M-4.3 trace bucket computation (item 2 of the audit) mis-mapped the
/// all-in action: with the old `stacks + street_bets` argument the jam
/// check `amount >= stacks + street_bets` compares against an inflated
/// threshold and the all-in lands in a fractional bucket instead of 6.
///
/// Dealer = 0 (SB), so OOP = seat 1 acts first postflop. Line:
/// P1 bets 30, P0 raises to 90, P1 (OOP, `street_bets = 30`,
/// stack 170) to act. Effective stack = 200 = the actor's all-in
/// total, so the tree root contains `Bet(200)`.
fn river_facing_raise() -> GameState {
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.set_hole_cards([0, 5], [10, 15]);
    act(&mut s, ActionKind::Call);
    act(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[20, 25, 30]);
    act(&mut s, ActionKind::Check);
    act(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[35]);
    act(&mut s, ActionKind::Check);
    act(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[44]);
    // River: P1 (OOP) bets 30, P0 raises to 90. P1 to act.
    act(&mut s, ActionKind::Bet(30.0));
    act(&mut s, ActionKind::Bet(90.0));
    assert_eq!(s.street, Street::River, "fixture must be river");
    assert_eq!(s.actor, 1, "fixture actor must be the OOP bettor");
    assert_eq!(s.actor, 1 - s.dealer, "actor must be OOP (root_actor = 0)");
    assert!(s.street_bets[s.actor] > 0.0, "fixture must face a raise");
    s
}

/// Uniform 1326-hand range over hands that share no card with the
/// board or `our`.
fn uniform_masked_range(st: &GameState, our: &[u8; 2]) -> Vec<f32> {
    let board = &st.board[..st.board_len as usize];
    let holes = pkr_subgame::vector::hole_table();
    let mut r = vec![0.0f32; pkr_subgame::vector::N];
    let mut n = 0usize;
    for h in 0..pkr_subgame::vector::N {
        let hh = holes[h];
        if hh.iter().any(|c| board.contains(c)) {
            continue;
        }
        if hh.iter().any(|c| our.contains(c)) {
            continue;
        }
        r[h] = 1.0;
        n += 1;
    }
    assert!(n > 100, "fixture sanity: nonempty range");
    let u = 1.0 / n as f32;
    for x in r.iter_mut() {
        if *x > 0.0 {
            *x = u;
        }
    }
    r
}

/// Audit item 2: the M-4.3 trace buckets must map the tree's all-in
/// action to JAM_BUCKET (6) even when the actor has chips already in
/// the street. The old code passed `stacks + street_bets` as
/// `action_bucket`'s `stacks` (chips BEHIND) parameter, inflating the
/// jam threshold: the all-in then fell into a fractional bucket (3 in
/// this fixture) and the tracker was updated with the wrong bucket.
#[test]
fn traced_bucket_marks_all_in_as_jam_when_facing_a_raise() {
    let st = river_facing_raise();
    // The actor is seat 1 (OOP); our holding is seat 1's hole.
    let our = st.hole[1];
    // Solver ranges are [OOP, IP]: OOP = us (point... uniform over the
    // tracker-style masked range), IP = seat 0's hole masked range.
    let r_oop = uniform_masked_range(&st, &our);
    let r_ip = uniform_masked_range(&st, &st.hole[0]);

    let tbl = CompactRegretTable::with_capacity(1);
    let cfg = VSearchCfg {
        bet_fracs: vec![0.5, 1.0],
        // Room for the river raise war so the tree's root keeps bet
        // actions (incl. the all-in) despite raises_this_street = 2.
        max_raises: 3,
        iters: 60,
        enabled_streets: [false, false, false, true],
        bound: true,
        gadget: false,
        bound_max_steps: 2,
        time_budget_ms: 0,
    };

    let (acts, trace) =
        vsearch::solve_root_bounded_traced(&st, [r_oop, r_ip], our, &cfg, &StubAbs, &tbl)
            .expect("river solve must succeed");
    assert!(!acts.is_empty(), "root strategy must be non-empty");

    let Some((root_acts, buckets, per_hand)) = trace else {
        panic!(
            "expected the kept-whole per-hand trace; the bound blended \
             toward the (uniform) blueprint so the tracker trace is gone"
        );
    };
    assert_eq!(root_acts.len(), buckets.len());
    assert_eq!(per_hand.len(), root_acts.len());

    let all_in_total = st.stacks[1] + st.street_bets[1]; // 200 chips
    let jam_idx = root_acts
        .iter()
        .position(
            |a| matches!(a, pkr_subgame::vector::Act::Bet(t) if (*t - all_in_total).abs() < 1e-4),
        )
        .expect("tree root must contain the all-in action");

    use pkr_core::abstraction::JAM_BUCKET;
    assert_eq!(
        buckets[jam_idx], JAM_BUCKET,
        "all-in Bet({all_in_total}) must map to the jam bucket; \
         got bucket {} (acts: {:?} -> {:?})",
        buckets[jam_idx], root_acts, buckets
    );
    assert_eq!(per_hand[jam_idx].len(), pkr_subgame::vector::N);
}

/// Serialize the env-var-dependent tests: the PKR_* flags are process
/// globals and the enabled-checks are OnceLocks, so tests that touch
/// them must not run concurrently within this binary.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ----------------------------------------------------------------------
// 2026-11-03 re-audit (item 6): corrupt (unsorted) key tables.
// ----------------------------------------------------------------------
//
// `MmapReader` used to accept any key table; an unsorted (corrupt) file
// silently degraded the binary search past the disorder point to misses.
// The fix: a startup-only order scan that WARNs loudly (load still
// succeeds) plus a `HealthReport.misses` field so a host can see the
// fallback-miss rate.

/// Build a minimal v2 blueprint whose key table is exactly `keys` (LE
/// u64s). Key index i gets a DISTINCT CDF (all bytes = i+1) so a test
/// can tell whose advice a lookup returned.
fn blueprint_with_keys(keys: &[u64], k: u8) -> Vec<u8> {
    use std::io::Write;
    let mut buf = Vec::new();
    let fh = pkr_export::header::FileHeader {
        magic: *b"PKRSOTA1",
        version: pkr_export::header::FORMAT_VERSION_V2,
        variant_id: 0,
        infoset_count: keys.len() as u64,
        max_actions_k: k,
        hash_algo: pkr_export::header::HASH_ALGO_FNV1A64_INFOSET,
        _padding: [0; 6],
    };
    buf.write_all(bytemuck::bytes_of(&fh)).unwrap();
    buf.write_all(&(keys.len() as u32).to_le_bytes()).unwrap();
    let cdf_len = (keys.len() * k as usize) as u32;
    buf.write_all(&cdf_len.to_le_bytes()).unwrap();
    for key in keys {
        buf.write_all(&key.to_le_bytes()).unwrap();
    }
    for (i, _) in keys.iter().enumerate() {
        buf.extend(std::iter::repeat_n(i as u8 + 1, k as usize));
    }
    buf
}

/// The loader must NOT reject an unsorted key table (warn, don't fail),
/// and lookups must degrade to MISS past the disorder point — never to
/// another key's advice.
#[test]
fn unsorted_key_table_loads_and_misses_never_wrong_advice() {
    // Sorted prefix [10], then disorder: 30 at index 1, 20 at index 2.
    let keys = [10u64, 30, 20];
    let data = blueprint_with_keys(&keys, 3);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &data).unwrap();

    // Loads without error (the loud warning goes to stderr).
    let reader = pkr_runtime::MmapReader::new(tmp.path())
        .expect("unsorted keys must still LOAD (warn, not fail)");
    let handle = pkr_runtime::SolverHandle::new(reader);

    // A key BEFORE the disorder point still resolves, with ITS OWN cdf.
    let a = handle.get_advice_fast(10).expect("pre-disorder key hits");
    assert_eq!(a.len, 3);
    assert!(
        a.cdf_probabilities[..3].iter().all(|&b| b == 1),
        "key 10 must return key 10's advice, got {:?}",
        &a.cdf_probabilities[..3]
    );

    // Keys AT/PAST the disorder point degrade to misses: key 30 is in
    // the table but the broken order makes the binary search miss it
    // (it lands on key 20, which != 30 -> None). Crucially the lookup
    // must never return key 20's cdf for query 30.
    let b = handle.get_advice_fast(30);
    assert!(
        b.is_none(),
        "query past the disorder point must miss, got {:?}",
        b.map(|x| x.cdf_probabilities[..3].to_vec())
    );
    assert!(
        handle.get_advice_fast(20).is_none(),
        "20 is past the disorder point too"
    );

    // The batch path walks keys and hashes together in sorted order, so
    // it may still find keys past the disorder point (its cursor is not
    // a binary search). The INVARIANT it must keep is the same: a hit is
    // always the EXACT queried key's advice, never another key's.
    let batch = handle.get_advice_batch(&[10, 30, 20, 99]);
    assert!(batch[0].is_some(), "pre-disorder key hits in batch too");
    assert!(batch[0].as_ref().unwrap().cdf_probabilities[..3]
        .iter()
        .all(|&b| b == 1));
    if let Some(a30) = &batch[1] {
        assert!(
            a30.cdf_probabilities[..3].iter().all(|&b| b == 2),
            "a batch hit for key 30 must be key 30's OWN advice"
        );
    }
    assert!(
        batch[2].is_none(),
        "key 20 cannot be found past the disorder"
    );
    assert!(batch[3].is_none(), "absent key misses");

    // Health check surfaces the fallback-miss count directly.
    let report = handle.health_check(64);
    assert_eq!(report.misses, report.queries - report.hits);
    assert!(report.hits > 0, "the stride sweep visits key 10 repeatedly");
    assert!(
        report.summary().contains("misses="),
        "summary names the miss count: {}",
        report.summary()
    );
}

/// Control: the SAME table sorted behaves normally — every key hits and
/// gets its own advice.
#[test]
fn sorted_key_table_hits_every_key() {
    let keys = [10u64, 20, 30];
    let data = blueprint_with_keys(&keys, 3);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), &data).unwrap();
    let reader = pkr_runtime::MmapReader::new(tmp.path()).unwrap();
    let handle = pkr_runtime::SolverHandle::new(reader);
    for (i, &k) in keys.iter().enumerate() {
        let a = handle.get_advice_fast(k).expect("sorted table hits");
        assert!(
            a.cdf_probabilities[..3].iter().all(|&b| b == i as u8 + 1),
            "key {k} must return its own advice"
        );
    }
    let report = handle.health_check(64);
    assert_eq!(report.misses, report.queries - report.hits);
    // Only the deliberate every-8th miss; no key-table degradation.
    assert!(
        report.misses <= report.queries / 8 + 1,
        "{}",
        report.summary()
    );
}

/// Build a RuntimeSession over a stub blueprint (uniform lookups). The
/// SubgameHandle has every street disabled, so `advise` can only answer
/// through the vsearch path — any Some/None comes from there.
fn make_vsearch_session<'a>(
    our_seat: u8,
    tbl: &'a CompactRegretTable,
    abs: &'a StubAbs,
    ev: &'a pkr_eval::NlheEvaluator,
) -> pkr_runtime::session::RuntimeSession<'a> {
    let handle = pkr_runtime::subgame::SubgameHandle::new(pkr_runtime::subgame::SubgameConfig {
        evaluator: std::sync::Arc::new(pkr_eval::NlheEvaluator),
        abstraction: std::sync::Arc::new(StubAbs),
        table: std::sync::Arc::new(CompactRegretTable::with_capacity(1)),
        iters: 1,
        hands_per_range: 4,
        enabled_streets: [false, false, false, false],
    });
    let abs_ref: &dyn AbstractionBuilder = abs;
    let ev_ref: &dyn pkr_contracts::Evaluator = ev;
    pkr_runtime::session::RuntimeSession::new(handle, our_seat, abs_ref, tbl, ev_ref)
}

/// River state, seat 1 (OOP) to act facing nothing — a clean vsearch
/// decision point for `our_seat = 1`.
fn river_our_seat_1() -> GameState {
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.set_hole_cards([0, 5], [10, 15]);
    let a = |st: &mut GameState, kind: ActionKind| {
        let p = st.actor;
        st.apply_action_in_place(&Action { player: p, kind });
    };
    a(&mut s, ActionKind::Call);
    a(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[20, 25, 30]);
    a(&mut s, ActionKind::Check);
    a(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[35]);
    a(&mut s, ActionKind::Check);
    a(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[44]);
    assert_eq!(s.actor, 1);
    assert_eq!(s.street, Street::River);
    s
}

/// Audit item 1: `PKR_VSEARCH_GADGET=1` must route the session's
/// `advise_vsearch` to the CFR-D gadgetted solve, not the raw one. The
/// old code set `cfg.gadget = want_gadget` but then branched only on
/// `cfg.bound`, so gadget mode silently ran the RAW unbounded
/// `solve_root` on the session path. The gadget entry point bumps
/// `VSEARCH_GADGET_SOLVES`; assert it moves.
#[test]
fn session_gadget_flag_routes_to_gadgetted_solver() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // NOTE: PKR_VSEARCH is read through a OnceLock (once per process);
    // every test in this file keeps it at "1". GADGET/RAW/BUDGET are
    // read per call.
    std::env::set_var("PKR_VSEARCH", "1");
    std::env::set_var("PKR_VSEARCH_GADGET", "1");
    std::env::remove_var("PKR_VSEARCH_RAW");
    std::env::set_var("PKR_VSEARCH_BUDGET_MS", "150");

    let before = vsearch::VSEARCH_GADGET_SOLVES.load(std::sync::atomic::Ordering::Relaxed);

    let tbl = CompactRegretTable::with_capacity(1);
    let abs = StubAbs;
    let ev = pkr_eval::NlheEvaluator;
    let mut sess = make_vsearch_session(1, &tbl, &abs, &ev);
    let st = river_our_seat_1();
    sess.deal_start(st.clone());
    let hole = st.hole[1];
    // The answer itself is not asserted (uniform blueprint => arbitrary
    // strategy); the ROUTING is what this test pins down.
    let _ = sess.advise(&st, &hole);

    let after = vsearch::VSEARCH_GADGET_SOLVES.load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        after > before,
        "PKR_VSEARCH_GADGET=1 must run the gadgetted solve \
         (VSEARCH_GADGET_SOLVES {before} -> {after})"
    );
}

/// Audit item 1 (control): without the gadget flag the session path
/// must NOT touch the gadget counter (default = bound mode).
#[test]
fn session_default_bound_mode_does_not_run_gadget() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("PKR_VSEARCH", "1");
    std::env::remove_var("PKR_VSEARCH_GADGET");
    std::env::remove_var("PKR_VSEARCH_RAW");
    std::env::set_var("PKR_VSEARCH_BUDGET_MS", "150");

    let before = vsearch::VSEARCH_GADGET_SOLVES.load(std::sync::atomic::Ordering::Relaxed);

    let tbl = CompactRegretTable::with_capacity(1);
    let abs = StubAbs;
    let ev = pkr_eval::NlheEvaluator;
    let mut sess = make_vsearch_session(1, &tbl, &abs, &ev);
    let st = river_our_seat_1();
    sess.deal_start(st.clone());
    let _ = sess.advise(&st, &st.hole[1]);

    let after = vsearch::VSEARCH_GADGET_SOLVES.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(after, before, "bound mode must not enter the gadget solver");
}

/// Turn spot, dealer = 0 so seat 1 (OOP) acts first postflop.
fn turn_our_seat_1_to_act() -> GameState {
    let mut s = GameState::new(200.0, 1.0, 2.0);
    s.set_hole_cards([0, 5], [10, 15]);
    let a = |st: &mut GameState, kind: ActionKind| {
        let p = st.actor;
        st.apply_action_in_place(&Action { player: p, kind });
    };
    a(&mut s, ActionKind::Call);
    a(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[20, 25, 30]);
    a(&mut s, ActionKind::Check);
    a(&mut s, ActionKind::Check);
    s.advance_street_in_place(&[35]);
    assert_eq!(s.street, Street::Turn);
    assert_eq!(s.actor, 1);
    s
}

/// Audit item 7: an out-of-order host action (`action.player` != the
/// tracked actor) must not silently update the wrong seat. The runtime
/// warns loudly and drops the action; the tracker state is unchanged,
/// and a subsequent well-ordered action still applies normally.
#[test]
fn observe_action_drops_mismatched_seat_without_corruption() {
    let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("PKR_VSEARCH", "1");

    let tbl = CompactRegretTable::with_capacity(1);
    let abs = StubAbs;
    let ev = pkr_eval::NlheEvaluator;
    let mut sess = make_vsearch_session(1, &tbl, &abs, &ev);
    let root = turn_our_seat_1_to_act();
    sess.deal_start(root.clone());

    let t0 = sess.tracker_state().unwrap();
    assert_eq!(t0.actor, 1);
    let (hist0, sb0, stacks0) = (t0.history_len, t0.street_bets, t0.stacks);
    let opp0 = *sess.opp_range().unwrap();

    // WRONG seat: the tracker's actor is 1, the host reports seat 0.
    sess.observe_action(Action {
        player: 0,
        kind: ActionKind::Check,
    });

    let t1 = sess.tracker_state().unwrap();
    assert_eq!(t1.actor, 1, "mismatched action must not advance the state");
    assert_eq!(
        t1.history_len, hist0,
        "mismatched action must not enter the history"
    );
    assert_eq!(t1.street_bets, sb0);
    assert_eq!(t1.stacks, stacks0);
    assert_eq!(*sess.opp_range().unwrap(), opp0, "posterior untouched");

    // A BET from the wrong seat must be dropped too (not applied as the
    // actor's bet).
    sess.observe_action(Action {
        player: 0,
        kind: ActionKind::Bet(50.0),
    });
    let t2 = sess.tracker_state().unwrap();
    assert_eq!(t2.actor, 1);
    assert_eq!(t2.street_bets, sb0, "mismatched bet must not move chips");
    assert_eq!(t2.stacks, stacks0);

    // The happy path is unaffected: the correctly-seated action applies.
    sess.observe_action(Action {
        player: 1,
        kind: ActionKind::Check,
    });
    let t3 = sess.tracker_state().unwrap();
    assert_eq!(
        t3.actor, 0,
        "well-ordered action must still advance the state"
    );

    // And a stale action aimed at the OLD seat (now the other actor)
    // must be dropped again.
    sess.observe_action(Action {
        player: 1,
        kind: ActionKind::Check,
    });
    let t4 = sess.tracker_state().unwrap();
    assert_eq!(t4.actor, 0, "stale duplicate must be dropped");
}
