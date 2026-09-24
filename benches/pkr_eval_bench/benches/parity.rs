//! Property test: TableEvaluator and NlheEvaluator must agree on
//! every (hole, board) input. Runs under the default #[test] harness
//! so the criterion suite is skipped but cargo test runs it.
//!
//! CI: invoked via `cargo test -p pkr-eval-bench --bench parity`.
//!
//! NOTE (worklog B6): the plan draft built cases from Card structs and
//! duplicated board cards on some iterations. The real evaluator takes
//! raw u8 ids and `slow.rs` dedups while the table path may not, so
//! all cases here use 7 unique cards from a deterministic shuffle.
//!
//! SCOPE NOTE (worklog B6): the repo's own differential tests
//! (`fast7_matches_slow_evaluator`) only prove agreement on full
//! 7-card boards (2 hole + 5 board). A 6-card probe during B6
//! (hole=[13,7] board=[32,30,42,19]) diverged:
//! table=4293495759 vs slow=4293495807. Root cause (read-only
//! analysis, NOT fixed — evaluator source is outside this plan):
//! `slow.rs` evaluates 6-card hands with
//! `COMBOS_7_5.iter().take(6)`, but the first 6 entries of that table
//! only cover the subsets that drop index 3, 4, or 5 — the subsets
//! dropping index 0, 1, or 2 (entries 7, 11, 16) are never evaluated,
//! so slow can return a worse-than-true rank on turn boards. The
//! enforced test below therefore covers full 7-card boards only; the
//! 6-card case is kept as an ignored regression marker.

use pkr_contracts::Evaluator;
use pkr_eval::TableEvaluator;
use pkr_eval::slow::NlheEvaluator;

/// Deterministic 7-unique-card deals: 2 hole + 5 board.
fn sample_cases() -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    // 50 deterministic hands: Fisher-Yates the first 9 slots of a
    // 52-card deck with an LCG, take 2 hole + 5 board.
    let mut state: u64 = 0x1234_5678_9ABC_DEF0;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    for _ in 0..50 {
        let mut deck: Vec<u8> = (0..52).collect();
        for i in 0..9 {
            let j = i + next() % (52 - i);
            deck.swap(i, j);
        }
        let hole = vec![deck[0], deck[1]];
        let board = deck[2..7].to_vec();
        out.push((hole, board));
    }
    out
}

#[test]
fn table_matches_slow_on_sample_cases() {
    let path =
        std::env::var("PKR_HAND_RANKS").expect("PKR_HAND_RANKS must point at hand_ranks.bin");
    let table = TableEvaluator::new(&path).unwrap();
    let slow = NlheEvaluator;

    for (hole, board) in sample_cases() {
        let t = table.evaluate_hand(&hole, &board);
        let s = slow.evaluate_hand(&hole, &board);
        assert_eq!(t, s, "mismatch on hole={hole:?} board={board:?}");
    }
}

/// Regression marker for the 6-card (turn-board) divergence found in
/// B6. Ignored until `slow.rs`'s `take(6)` subset bug is fixed; see
/// the SCOPE NOTE above. If this starts passing, the bug is fixed —
/// un-ignore it and delete this comment.
#[test]
#[ignore]
fn table_matches_slow_on_turn_boards() {
    let path =
        std::env::var("PKR_HAND_RANKS").expect("PKR_HAND_RANKS must point at hand_ranks.bin");
    let table = TableEvaluator::new(&path).unwrap();
    let slow = NlheEvaluator;

    let hole = vec![13u8, 7u8];
    let board = vec![32u8, 30u8, 42u8, 19u8];
    let t = table.evaluate_hand(&hole, &board);
    let s = slow.evaluate_hand(&hole, &board);
    assert_eq!(t, s, "turn-board mismatch on hole={hole:?} board={board:?}");
}
