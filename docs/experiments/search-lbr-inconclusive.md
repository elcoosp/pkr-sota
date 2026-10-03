# Search inside LBR: inconclusive at current settings (2026-10-03)

**Status:** not a demonstrated win. The gadget is *provably safe*
(`gadget_br <= bp_br`); its *usefulness* is unmeasured.

## Measurement

| arm | LBR mbb/hand | hands | SE |
|---|---|---|---|
| blueprint only | 1115.6 | 20k | 109.6 |
| + search (river+turn, gadget, 50 iters, 12 hands/range) | 1030.2 | 5k | 214.2 |

Δ = -85 mbb, combined SE ~306 -> **0.28 sigma. Noise.**

## Cost

Search LBR: ~30 min / 5k hands. No-search: ~12 min / 20k.
**~10x slower per hand** (a subgame solve per bot decision).

## Why it may not show

1. Solve settings are thin: 50 iterations, 12 hands/range. The river POC
   used 100 iters and still warned real (tighter) ranges shrink the
   measured gain from 59% to 20-40%; here we see ~8%, inside noise.
2. Ranges come from the tracker WITHOUT translation (`PKR_TRANSLATE` is
   off), so off-tree opponent sizes are still hard-thresholded.
3. Only river+turn enabled; flop search not tried.

## What a real test needs

- >=20k hands with search (~2h per checkpoint), AND
- stronger solve: >=100 iters, larger range sample, translation on.

Only then is an 85-mbb effect distinguishable. Until then: the safety
property holds; the strength claim does not.
