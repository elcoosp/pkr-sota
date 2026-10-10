# Fix α: 2 → 1.5 in the integer discount path

## The bug

`crates/pkr-cfr/src/dcfr.rs:180` (`discount_pos_i64`) computes:

    d = t² + 1
    r' = r - ceil(r / d)   ===   floor(r · t² / (t² + 1))

The exponent on `t` is 2. The DCFR paper (Brown & Sandholm, AAAI 2019)
recommends (α, β, γ) = (1.5, 0, 2). We've been running α=2 under an
α=1.5 label since the i64 rewrite.

The comment at `dcfr.rs:158-159` claims the paper says "any α ∈ [1, 2]
gives similar results". The paper does not say that; it says the
specific tuple (1.5, 0, 2) is the recommended default and consistently
outperforms CFR+ in practice.

## Fix

Change the integer discount to compute α=1.5. Since `t^1.5 = t·√t`
is irrational, we compute it in f64 and apply the result to the i64
regret with a single multiply — cheaper than the integer division we
currently do.

## Where to change

`crates/pkr-cfr/src/dcfr.rs`:

1. Add a precomputed `DcfrStep` (mirrors the plan from an earlier
   session that didn't land):
   ```rust
   pub struct DcfrStep {
       pub w_pos: f64,   // t^1.5 / (t^1.5 + 1)
       pub w_neg: f64,   // t^0 / (t^0 + 1) = 0.5
       pub gamma: f64,   // 1 / sqrt(t+1)
   }
   pub fn dcfr_step(iteration: u32) -> DcfrStep { ... }
