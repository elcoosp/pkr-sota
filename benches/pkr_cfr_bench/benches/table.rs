//! Benchmark CompactRegretTable public ops: strategy lookup,
//! snapshot, analyze, sample. Capacity = 100_000 (small enough to fit
//! in L2, isolating logic cost from memory-bandwidth cost).
//!
//! NOTE (worklog B8): the plan draft called `get_or_create_idx`, which
//! is `pub(crate)` in the real `table.rs` — unreachable from an
//! external bench crate. Insertion is exercised through the public
//! `get_strategy_and_idx(hash, &mut out, &mut LocalMetrics)` instead,
//! which performs the same get-or-create path plus regret-matching.
//! `get_strategy_into` covers the pure-lookup path.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pkr_cfr::metrics::LocalMetrics;
use pkr_cfr::table::CompactRegretTable;

fn fill_table(t: &CompactRegretTable, n: u64) {
    let mut out = [0.0f32; 6];
    let mut m = LocalMetrics::default();
    for i in 0..n {
        let k = i.wrapping_mul(0x9E3779B97F4A7C15);
        let _ = t.get_strategy_and_idx(k, &mut out, &mut m);
    }
}

fn bench_get_or_create(c: &mut Criterion) {
    let mut group = c.benchmark_group("table/get_strategy_and_idx");
    for n in [100_u64, 1_000, 100_000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(n), n, |b, &n| {
            b.iter_batched(
                || CompactRegretTable::with_capacity(200_000),
                |t| {
                    let mut out = [0.0f32; 6];
                    let mut m = LocalMetrics::default();
                    for i in 0..n {
                        let k = i.wrapping_mul(0x9E3779B97F4A7C15);
                        let _ = black_box(t.get_strategy_and_idx(k, &mut out, &mut m));
                    }
                },
                criterion::BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_lookup_hit(c: &mut Criterion) {
    let t = CompactRegretTable::with_capacity(200_000);
    fill_table(&t, 100_000);
    c.bench_function("table/get_strategy_into_hit/100k", |b| {
        b.iter(|| {
            let mut out = [0.0f32; 6];
            // Key 42*GOLDEN was inserted by fill_table -> lookup hit.
            t.get_strategy_into(black_box(42u64.wrapping_mul(0x9E3779B97F4A7C15)), &mut out);
            black_box(out)
        })
    });
}

fn bench_snapshot(c: &mut Criterion) {
    let t = CompactRegretTable::with_capacity(200_000);
    fill_table(&t, 100_000);
    c.bench_function("table/snapshot/100k", |b| {
        b.iter(|| black_box(t.snapshot()))
    });
}

fn bench_analyze(c: &mut Criterion) {
    let t = CompactRegretTable::with_capacity(200_000);
    fill_table(&t, 100_000);
    c.bench_function("table/analyze_strategies/100k", |b| {
        b.iter(|| black_box(t.analyze_strategies()))
    });
}

fn bench_sample(c: &mut Criterion) {
    let t = CompactRegretTable::with_capacity(200_000);
    fill_table(&t, 100_000);
    c.bench_function("table/sample_infosets/200", |b| {
        b.iter(|| black_box(t.sample_infosets(black_box(200))))
    });
}

criterion_group!(
    benches,
    bench_get_or_create,
    bench_lookup_hit,
    bench_snapshot,
    bench_analyze,
    bench_sample
);
criterion_main!(benches);
