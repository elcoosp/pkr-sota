//! Benchmark GameState::legal_actions_into — the per-node allocator.
//!
//! NOTE (worklog B5): the plan draft used `GameState::new_heads_up()`
//! and `legal_actions_into(&mut [u8; 16])`. The real API is
//! `GameState::new(start_stack, sb, bb)` and
//! `legal_actions_into(&mut [Action; 8]) -> usize`. This bench uses
//! the real signatures.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pkr_core::state::{Action, ActionKind, GameState};

fn bench_legal_actions(c: &mut Criterion) {
    let state = GameState::new(200.0, 1.0, 2.0);
    c.bench_function("state/legal_actions_into", |b| {
        b.iter(|| {
            let mut buf = [Action {
                player: 0,
                kind: ActionKind::Fold,
            }; 8];
            let n = state.legal_actions_into(black_box(&mut buf));
            black_box(n)
        })
    });
}

criterion_group!(benches, bench_legal_actions);
criterion_main!(benches);
