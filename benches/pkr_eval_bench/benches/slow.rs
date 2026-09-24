//! Benchmark the slow path (no lookup tables). This is the parity
//! reference for the fast path; it must not silently diverge.
//!
//! NOTE (worklog B6): the plan draft called `NlheEvaluator::new()`.
//! The real `NlheEvaluator` is a unit struct — use `NlheEvaluator`
//! directly. Card ids are raw u8 (0..52); see table.rs note.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pkr_contracts::Evaluator;
use pkr_eval::slow::NlheEvaluator;

fn make_evaluator() -> NlheEvaluator {
    NlheEvaluator
}

fn sample_hole() -> [u8; 2] {
    [48, 44]
}

fn sample_board() -> [u8; 5] {
    [0, 5, 10, 15, 20]
}

fn bench_evaluate_preflop(c: &mut Criterion) {
    let e = make_evaluator();
    let hole = sample_hole();
    let board: Vec<u8> = vec![];
    c.bench_function("slow_eval/preflop", |b| {
        b.iter(|| black_box(e.evaluate_hand(black_box(&hole), black_box(&board))))
    });
}

fn bench_evaluate_river(c: &mut Criterion) {
    let e = make_evaluator();
    let hole = sample_hole();
    let board = sample_board();
    c.bench_function("slow_eval/river", |b| {
        b.iter(|| black_box(e.evaluate_hand(black_box(&hole), black_box(&board))))
    });
}

criterion_group!(benches, bench_evaluate_preflop, bench_evaluate_river);
criterion_main!(benches);
