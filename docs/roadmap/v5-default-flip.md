# Scoping: make V5 the default signature

**Status:** recommended by the 2026-10-06 A/B; NOT yet executed.

## The evidence

`docs/experiments/v4-vs-v5-ab-result.md`: V5 beat V4 by 823 mbb LBR
(|t| ≈ 6.3) and 10.8 bb/100 arena. Three instruments agree.

## Why it is not a one-line change

The default signature is currently **V3**, set by
`crates/pkr-core/src/state.rs::sig_v3_size_aware()`:

```rust
std::env::var("PKR_SIG_V3").as_deref() != Ok("0")   // true unless PKR_SIG_V3=0
