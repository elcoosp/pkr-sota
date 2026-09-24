//! Benchmark the Deck deal path (the part called per-traverse).
//!
//! NOTE (worklog B5): the plan draft called `deck.deal_one() -> u8`.
//! The real API is `Deck::deal() -> Option<Card>`. This bench deals
//! 5 cards per iteration through the real API.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pkr_core::deck::Deck;

fn bench_deck_deal_5(c: &mut Criterion) {
    c.bench_function("deck/deal_5", |b| {
        b.iter_batched(
            Deck::new,
            |mut deck| {
                let mut n = 0u32;
                for _ in 0..5 {
                    if black_box(deck.deal()).is_some() {
                        n += 1;
                    }
                }
                n
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, bench_deck_deal_5);
criterion_main!(benches);
