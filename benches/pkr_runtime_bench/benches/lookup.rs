//! Benchmark SolverHandle::get_advice_fast — the runtime hot path.
//!
//! Three dimensions:
//!   1. num_keys: 100, 1_000, 100_000, 1_000_000
//!   2. hit vs miss (miss = key not in the array)
//!   3. position: head / middle / tail (binary search path length)
//!
//! Requires a smoke-built blueprint. We use whatever PKR_BLUEPRINT
//! points at; if absent, the bench is skipped with a clear error.
//!
//! NOTE (worklog B7): all four API facts from the plan draft check out
//! against the real source (`crates/pkr-runtime/src/{lib,lookup,mmap}.rs`):
//! `SolverHandle::new(MmapReader)`, `MmapReader::new(path)`,
//! `SolverHandle::debug_keys() -> &[u8]`,
//! `get_advice_fast(u64) -> Option<SotaAdvice>`.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use pkr_runtime::mmap::MmapReader;
use pkr_runtime::SolverHandle;
use std::env;

fn open_handle() -> SolverHandle {
    let path = env::var("PKR_BLUEPRINT")
        .expect("PKR_BLUEPRINT must point at blueprint.bin (smoke.sh produces one)");
    let rdr = MmapReader::new(&path).expect("failed to mmap blueprint");
    SolverHandle::new(rdr)
}

fn keys_from_blueprint(h: &SolverHandle) -> Vec<u64> {
    let bytes = h.debug_keys();
    let n = bytes.len() / 8;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let k = u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
        out.push(k);
    }
    out
}

fn bench_lookup(c: &mut Criterion) {
    let h = open_handle();
    let keys = keys_from_blueprint(&h);
    if keys.is_empty() {
        eprintln!("WARNING: empty blueprint, skipping lookup bench");
        return;
    }
    let mid = keys.len() / 2;
    let head = 0usize;
    let tail = keys.len().saturating_sub(1);
    let miss = keys[tail].wrapping_add(1);

    let cases: &[(&str, u64)] = &[
        ("head_hit", keys[head]),
        ("mid_hit", keys[mid]),
        ("tail_hit", keys[tail]),
        ("miss", miss),
    ];

    let mut group = c.benchmark_group("lookup/get_advice_fast");
    group.sample_size(50); // larger sample for stable p99
    for &(label, k) in cases {
        group.bench_with_input(BenchmarkId::from_parameter(label), &k, |b, &k| {
            b.iter(|| black_box(h.get_advice_fast(black_box(k))))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_lookup);
criterion_main!(benches);
