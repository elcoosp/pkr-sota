//! Benchmark GlobalMetrics::record_batch + snapshot + delta.
//!
//! NOTE (worklog B8): the plan draft called `GlobalMetrics::new()`,
//! which is private in the real `metrics.rs` — the process-wide table
//! is exposed via the `global()` singleton (`OnceLock`). This bench
//! uses `global()`. The real `record_batch` signature is
//! `(m, iterations, wall_ns, traverse_ns, merge_ns, flush_ns,
//! regret_in, regret_out, strategy_in)` — 8 u64s after `m`, not the
//! 9-arg draft in the plan. `Snapshot::delta(&self, prev)` matches.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pkr_cfr::metrics::{global, LocalMetrics};

fn bench_record_batch(c: &mut Criterion) {
    let g = global();
    let lm = LocalMetrics::default();
    c.bench_function("metrics/record_batch", |b| {
        b.iter(|| {
            g.record_batch(
                black_box(&lm),
                black_box(256),
                black_box(1_000_000),
                black_box(900_000),
                black_box(50_000),
                black_box(50_000),
                black_box(1_000),
                black_box(500),
                black_box(2_000),
            );
        })
    });
}

fn bench_snapshot_delta(c: &mut Criterion) {
    let g = global();
    let lm = LocalMetrics::default();
    g.record_batch(
        &lm, 256, 1_000_000, 900_000, 50_000, 50_000, 1_000, 500, 2_000,
    );
    let prev = g.snapshot();
    c.bench_function("metrics/snapshot+delta", |b| {
        b.iter(|| {
            let cur = g.snapshot();
            black_box(cur.delta(&prev))
        })
    });
}

fn bench_record_node(c: &mut Criterion) {
    c.bench_function("metrics/record_node", |b| {
        b.iter_batched(
            LocalMetrics::default,
            |mut lm| {
                for d in 0..64u32 {
                    lm.record_node(black_box(d % 24));
                }
                lm.nodes
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    benches,
    bench_record_batch,
    bench_snapshot_delta,
    bench_record_node
);
criterion_main!(benches);
