//! Benchmark KMeansAbstraction::get_infoset_hash per street.
//!
//! NOTE (worklog B9): all three API facts check out against the real
//! source (`crates/pkr-abstraction/src/lib.rs`,
//! `crates/pkr-contracts/src/lib.rs`): `load_centroids(&str)`,
//! `KMeansAbstraction::from_store(store, Arc<dyn Evaluator>)`,
//! `init_table(street_code: u8, path: &str)`, and the trait method
//! `AbstractionBuilder::get_infoset_hash(&self, hole, board, history,
//! street)` (imported here so the method resolves).

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pkr_abstraction::{load_centroids, KMeansAbstraction};
use pkr_contracts::AbstractionBuilder;
use pkr_eval::TableEvaluator;
use std::env;
use std::sync::Arc;

fn make_abstraction() -> KMeansAbstraction {
    let p = env::var("PKR_HAND_RANKS").expect("PKR_HAND_RANKS");
    let ev = TableEvaluator::new(&p).unwrap();
    let store = load_centroids(&env::var("PKR_CENTROIDS").expect("PKR_CENTROIDS")).unwrap();
    let a = KMeansAbstraction::from_store(store, Arc::new(ev));
    // load each street table if env vars are present
    for (street, var) in [
        (0u8, "PKR_PREFLOP_TABLE"),
        (1, "PKR_FLOP_TABLE"),
        (2, "PKR_TURN_TABLE"),
        (3, "PKR_RIVER_TABLE"),
    ] {
        if let Ok(p) = env::var(var) {
            let _ = a.init_table(street, &p);
        }
    }
    a
}

fn bench_per_street(c: &mut Criterion) {
    let a = make_abstraction();
    let hole = [48u8, 44u8]; // As Ks as raw card ids
    let board_empty: Vec<u8> = vec![];
    let board_flop = vec![2u8, 3, 4];
    let board_turn = vec![2u8, 3, 4, 5];
    let board_river = vec![2u8, 3, 4, 5, 6];
    let history = b"cp"; // check-preflop

    let cases: &[(u8, &[u8], &str)] = &[
        (0, &board_empty, "preflop"),
        (1, board_flop.as_slice(), "flop"),
        (2, board_turn.as_slice(), "turn"),
        (3, board_river.as_slice(), "river"),
    ];

    let mut group = c.benchmark_group("abstraction/get_infoset_hash");
    for &(street, board, label) in cases {
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &(street, board),
            |b, &(street, board)| {
                b.iter(|| {
                    black_box(a.get_infoset_hash(
                        black_box(&hole),
                        black_box(board),
                        black_box(history),
                        black_box(street),
                    ))
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_per_street);
criterion_main!(benches);
