//! Benchmark FNV-1a 64-bit. This is called on every infoset hash,
//! so ~7.5M times/sec during training. A 50 ns regression here costs
//! ~375 ms/sec of throughput.
//!
//! Run: cargo bench -p pkr-contracts-bench --bench fnv1a

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pkr_contracts::{fnv1a, FNV_OFFSET};

fn bench_fnv1a_small(c: &mut Criterion) {
    // 8-byte input — typical for a single u64 history byte slice.
    let input = b"abcdefgh";
    c.bench_function("fnv1a/u64_input", |b| {
        b.iter(|| {
            let mut h = FNV_OFFSET;
            fnv1a(&mut h, black_box(input));
            black_box(h)
        })
    });
}

fn bench_fnv1a_varying(c: &mut Criterion) {
    // Real infoset hashes mix: 2-byte hole + 5-byte board + N-byte history.
    // Sizes: preflop = 2+0+~8, flop = 2+3+~8, turn = 2+4+~8, river = 2+5+~8.
    let sizes: &[(usize, &str)] = &[
        (10, "preflop"),
        (13, "flop"),
        (14, "turn"),
        (15, "river"),
    ];
    let mut group = c.benchmark_group("fnv1a/by_street");
    for &(n, label) in sizes {
        let input: Vec<u8> = (0..n).map(|i| i as u8).collect();
        group.bench_with_input(BenchmarkId::from_parameter(label), &input, |b, data| {
            b.iter(|| {
                let mut h = FNV_OFFSET;
                fnv1a(&mut h, black_box(data));
                black_box(h)
            })
        });
    }
    group.finish();
}

criterion_group!(benches, bench_fnv1a_small, bench_fnv1a_varying);
criterion_main!(benches);
