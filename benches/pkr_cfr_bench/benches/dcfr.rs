//! Benchmark the DCFR discount factor computation. The math is
//! per-infoset, so a regression here scales with the number of
//! infosets touched per iteration (~280 nodes/iter × ~27K it/s).
//!
//! NOTE (worklog B8): the plan draft called
//! `DiscountMode::CanonicalDcfr.weight(t, tau)`. The real API is the
//! free function `discount_factor_mode(t: f32, p: f32, mode)` (plus
//! `discount_factor(t, p)` for the production default). This bench
//! exercises the real function at production exponents.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pkr_cfr::dcfr::{discount_factor, discount_factor_mode, DiscountMode, ALPHA, BETA, GAMMA};

fn bench_discount_canonical(c: &mut Criterion) {
    c.bench_function("dcfr/canonical/t=1e6", |b| {
        b.iter(|| {
            let w = discount_factor_mode(
                black_box(1_000_000.0),
                black_box(ALPHA),
                DiscountMode::CanonicalDcfr,
            );
            black_box(w)
        })
    });
}

fn bench_discount_all_modes(c: &mut Criterion) {
    c.bench_function("dcfr/all_modes/t=1e6", |b| {
        b.iter(|| {
            let a = discount_factor(black_box(1_000_000.0), black_box(ALPHA));
            let g = discount_factor(black_box(1_000_000.0), black_box(GAMMA));
            let n = discount_factor_mode(black_box(1_000_000.0), black_box(BETA), DiscountMode::None);
            black_box((a, g, n))
        })
    });
}

criterion_group!(
    benches,
    bench_discount_canonical,
    bench_discount_all_modes
);
criterion_main!(benches);
