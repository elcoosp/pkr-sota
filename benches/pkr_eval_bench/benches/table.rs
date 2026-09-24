//! Benchmark TableEvaluator::evaluate_hand.
//!
//! Requires a hand_ranks.bin — point PKR_HAND_RANKS at it.
//!
//! NOTE (worklog B6): the plan draft built hole/board via
//! `Card::new(..).to_u8()`. The real `Card` has no `to_u8`, and the
//! `Evaluator` trait takes raw `&[u8]` card ids (0..52). This bench
//! passes raw ids (As/Ks hole, 2d3c4s5h6d board) through the real API.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pkr_contracts::Evaluator;
use pkr_eval::TableEvaluator;
use std::env;

fn make_evaluator() -> TableEvaluator {
    let path =
        env::var("PKR_HAND_RANKS").expect("PKR_HAND_RANKS must point at hand_ranks.bin");
    TableEvaluator::new(&path).expect("failed to load hand_ranks.bin")
}

// As = 12*4+0 = 48, Ks = 11*4+0 = 44 (suit-major: card = rank*4+suit
// with Spade=0). Exact ids don't matter, only that they are valid.
fn sample_hole() -> [u8; 2] {
    [48, 44]
}

fn sample_board() -> [u8; 5] {
    [0, 5, 10, 15, 20]
}

fn bench_evaluate_preflop(c: &mut Criterion) {
    let e = make_evaluator();
    let hole = sample_hole();
    let board: Vec<u8> = vec![]; // preflop
    c.bench_function("table_eval/preflop", |b| {
        b.iter(|| black_box(e.evaluate_hand(black_box(&hole), black_box(&board))))
    });
}

fn bench_evaluate_river(c: &mut Criterion) {
    let e = make_evaluator();
    let hole = sample_hole();
    let board = sample_board();
    c.bench_function("table_eval/river", |b| {
        b.iter(|| black_box(e.evaluate_hand(black_box(&hole), black_box(&board))))
    });
}

criterion_group!(benches, bench_evaluate_preflop, bench_evaluate_river);
criterion_main!(benches);
