//! Benchmark Card::new. Cards are constructed millions of times
//! per training second (deck setup, hole assignment).
//!
//! NOTE (worklog B5): the plan draft used `mem::transmute` to build
//! Rank/Suit from u8. The real `Rank`/`Suit` enums are `#[repr(u8)]`
//! with discriminants 0..13 / 0..4, so transmute would work, but the
//! explicit static arrays below are clearer and clippy-clean.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pkr_core::card::{Card, Rank, Suit};

const RANKS: [Rank; 13] = [
    Rank::Two,
    Rank::Three,
    Rank::Four,
    Rank::Five,
    Rank::Six,
    Rank::Seven,
    Rank::Eight,
    Rank::Nine,
    Rank::Ten,
    Rank::Jack,
    Rank::Queen,
    Rank::King,
    Rank::Ace,
];
const SUITS: [Suit; 4] = [Suit::Spade, Suit::Heart, Suit::Diamond, Suit::Club];

fn bench_card_new(c: &mut Criterion) {
    c.bench_function("card/new", |b| {
        b.iter(|| {
            for &r in &RANKS {
                for &s in &SUITS {
                    black_box(Card::new(s, r));
                }
            }
        })
    });
}

criterion_group!(benches, bench_card_new);
criterion_main!(benches);
