//! Golden-number test for `pkr_subgame::vector::Solver::exploitability`.
//!
//! The M-31 refactor (replace `unsafe { &mut *node }` with an arena of
//! u32 indices) is a pure structural change: the tree shape, the CFR
//! iterations, and the arithmetic must all be bit-identical before and
//! after. `exploitability()` is a very sensitive scalar summary of that
//! whole pipeline, so pinning it to a tolerance gives a strong safety
//! net that the arena refactor cannot land silently broken.
//!
//! This file currently has `#[ignore]` on the assertion so the FIRST
//! run records the value without failing. Once the value is captured,
//! remove `#[ignore]` and hard-code it.

use pkr_subgame::vector::{Solver, Spec, Act};

/// Deterministic evaluator that ignores the actual cards and uses the
/// sum of the two hole-card ids as the hand strength. Fast, no deps,
/// and not trivial (different hands do differ).
fn det_eval(hole: &[u8; 2], _board: &[u8]) -> u32 {
    (hole[0] as u32) * 31 + (hole[1] as u32)
}

fn build_river_solver(seed: u64) -> Solver<'static> {
    // Board: Ah Kd 7c 2h 9d (arbitrary five distinct cards).
    let board: Vec<u8> = vec![12, 13 + 11, 39 + 5, 13 + 0, 26 + 7];
    // Deterministic ranges weighted over all 1326 hands with a cheap
    // LCG; ~55% of hands are "in range".
    let mut s = seed;
    let mut lcg = || { s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); ((s >> 33) as f32) / ((1u64 << 31) as f32) };
    let mut r0 = vec![0.0f32; 1326];
    let mut r1 = vec![0.0f32; 1326];
    for h in 0..1326 {
        r0[h] = if lcg() < 0.55 { lcg() } else { 0.0 };
        r1[h] = if lcg() < 0.55 { lcg() } else { 0.0 };
    }
    let spec = Spec {
        board,
        dead: 100.0,
        stack: 150.0,
        root_actor: 0,
        root_inv: [0.0, 0.0],
        bet_fracs: vec![0.5, 1.0],
        max_raises: 2,
        min_bet: 2.0,
        allin_snap: 0.2,
        ranges: [r0, r1],
    };
    // Safety: the closure is `'static` (function pointer).
    let eval: &'static (dyn Fn(&[u8; 2], &[u8]) -> u32 + Sync) = &det_eval;
    Solver::new(spec, eval)
}

/// Captured 2026-10-11 from the pre-arena-refactor solver:
///   exploitability@50 = 0.658723, root_actions = 4.
/// If this number moves during the M-31 arena refactor, the refactor
/// is NOT behaviour-preserving. Small float drift from a
/// purely-structural change should be zero; any change at all is a bug.
#[test]
fn vector_exploitability_golden_50_iters() {
    let mut s = build_river_solver(7);
    s.iterate(50);
    let e = s.exploitability();
    eprintln!("VECTOR_GOLDEN exploitability@50 = {:.6}", e);
    assert!(e.is_finite(), "exploitability non-finite: {e}");
    assert!(e >= 0.0, "exploitability negative: {e}");
    // Tight tolerance: this number is deterministic (no randomness in
    // iterate/exploitability), so bit-identical output is expected.
    // 1e-4 gives headroom for a compiler change without masking a real
    // regression in the tree.
    let want = 0.658723_f32;
    assert!(
        (e - want).abs() < 1e-4,
        "exploitability drifted: got {e:.6}, want {want:.6}. \
         The M-31 arena refactor must be behaviour-preserving."
    );
    assert_eq!(
        s.n_root_actions(), 4,
        "root action count changed -- tree shape drifted"
    );
    // Sample one root action for shape.
    let acts: Vec<Act> = s.root_strategy([12, 13]).into_iter().map(|(a, _)| a).collect();
    assert_eq!(acts.len(), 4, "root_strategy returned the wrong action count");
}

#[test]
fn vector_solver_constructs_and_iterates() {
    // Non-ignored smoke test so the file exercises its own helpers on
    // every run. Also a cheap early warning if `Spec` changes shape.
    let mut s = build_river_solver(99);
    assert!(s.n_root_actions() > 0);
    s.iterate(3);
    let e = s.exploitability();
    assert!(e.is_finite());
}

/// Turn-tree golden. The board has 4 cards, so `end_street` builds a
/// Chance node over 46 rivers -- this is the code path P7 (reach-clone
/// elimination) touches. The river golden above does not exercise any
/// Chance node, so this test is the one that catches a chance-arm bug.
///
/// Same deterministic evaluator and LCG ranges as the river golden;
/// only the board length differs.
#[test]
fn vector_turn_golden_exercises_chance_nodes() {
    let board: Vec<u8> = vec![12, 24, 37, 50]; // 4-card turn board
    let mut s = 7u64;
    let mut lcg = || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((s >> 33) as f32) / ((1u64 << 31) as f32)
    };
    let mut r0 = vec![0.0f32; 1326];
    let mut r1 = vec![0.0f32; 1326];
    for h in 0..1326 {
        r0[h] = if lcg() < 0.55 { lcg() } else { 0.0 };
        r1[h] = if lcg() < 0.55 { lcg() } else { 0.0 };
    }
    let spec = Spec {
        board,
        dead: 100.0,
        stack: 150.0,
        root_actor: 0,
        root_inv: [0.0, 0.0],
        bet_fracs: vec![0.5, 1.0],
        max_raises: 2,
        min_bet: 2.0,
        allin_snap: 0.2,
        ranges: [r0, r1],
    };
    let eval: &'static (dyn Fn(&[u8; 2], &[u8]) -> u32 + Sync) = &det_eval;
    let mut solver = Solver::new(spec, eval);
    // Small iteration count: the turn tree is ~46x the river tree, and
    // each iteration walks every chance branch.
    solver.iterate(5);
    let e = solver.exploitability();
    eprintln!("VECTOR_TURN_GOLDEN exploitability@5 = {:.6}", e);
    assert!(e.is_finite(), "turn exploitability non-finite: {e}");
    assert!(e >= 0.0);
    // Captured 2026-10-11, before the P7 reach-clone refactor:
    //   turn exploitability@5 = 26.393711.
    // The value is deterministic, so 1e-4 tolerance catches any drift
    // from the P7 change to the Chance-arm reach handling.
    let want = 26.393711_f32;
    assert!(
        (e - want).abs() < 1e-4,
        "turn golden drifted: got {e:.6}, want {want:.6}. The P7 refactor \
must preserve per-hand arithmetic at Chance nodes."
    );
}
