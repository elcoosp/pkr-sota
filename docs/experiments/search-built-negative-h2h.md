# Search-as-built is a net NEGATIVE (H2H, 2026-10-06)

The sampled-hand subgame search (`pkr-subgame`, 4-12 hands, 10-50 iters)
loses head-to-head against the raw blueprint it was meant to improve.

| metric | value |
|---|---|
| A (search) - B (blueprint), same V4 ckpt | **-6.06 chips/deal** |
| SE | 0.97 |
| t | **-6.26** |
| hands | 3000, seats swapped, same deals |

## Why LBR said the opposite

LBR (-627 mbb) is a greedy, myopic exploiter. It cannot punish a
strategy that is unusual-but-not-locally-losing, so it scored the
searched policy as "harder to exploit" when it actually plays worse.
**H2H is the honest instrument; LBR alone is not.**

## Why search loses

1. Solves with a UNIFORM opponent range (tournament has no tracker).
2. 4-12 sampled hands / 10-50 iterations is far too coarse.
3. `PKR_SUBGAME_MAX_RIVERS=6` samples rivers, weakening it further.

## Consequence

The sampled-hand search is PARKED. Future search must be the
vector-form solver (Appendix A of the plan): full 1326-hand ranges,
exact river enumeration, 100s of iterations, gadget safety.
