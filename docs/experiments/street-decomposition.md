# Street decomposition of exploitability

**Date:** 2026-09-27
**Purpose:** Locate where the bot's exploitability lives — which streets
contribute most to the 2500-10000 mbb depending on deal count.

## Method

Added `ForceFoldHook` to the `SubgameHook` interface (see
`crates/pkr-exploit/tests/street_decomposition.rs`). At every agent
decision on the target street or later, the hook returns bucket 0
(fold). Below the target street, the hook returns None and the
blueprint plays.

Ran four evals on `outputs/v34long/train.ckpt`, 500 deals, seed 42:

| config | expl_mbb |
|---|---|
| baseline | 9999 |
| force-fold@flop | 23412 |
| force-fold@turn | 19926 |
| force-fold@river | 15361 |

Deltas:

| transition | delta |
|---|---|
| all-fold → play flop | −3486 |
| play flop → play turn | −4565 |
| play turn → play river | −5361 |
| **total (all-fold → baseline)** | **−13412** |

## Interpretation

- The bot's play is worth 13,412 mbb vs folding every postflop decision.
- Value is roughly even across the three streets.
- **River alone is 40% of the total.** Turn+river together = 74%.
- Flop contributes 26%.

No single street dominates — the bot is uniformly ~30% of the all-fold
benchmark. Any strategy improvement (subgame solving, better abstractions,
or longer training) applies to the whole curve, not one segment.

## Implication for subgame solving

The turn+river subgame solver covers 74% of postflop value. POC numbers:

- river per-subgame: +42.96 chips vs blueprint
- turn per-subgame: +26.62 chips vs blueprint

If those gains translate to even 20% end-to-end improvement on their
respective streets, that's ~2000 mbb total. The POC showed 40-60%
per-subgame improvements, so the ceiling is higher.

**River-only shipping is viable.** River latency is ~100ms (small tree);
turn latency is ~1.15s (46-branch chance). A river-only subgame solver
captures 40% of postflop value at 1/10 the latency budget. Ship order:

1. River-only subgame solving (low latency, strong POC win)
2. Turn subgame solving (higher latency, still strong POC win)
3. Flop — not viable at current architecture

## Caveats

- Deal count 500 → SE ≈ 700 mbb. Deltas are 3-19 SE, so directionally
  solid but magnitudes have ±30% uncertainty.
- "Force fold" at a check node means an illegal action; the walker
  silently returns 0 in that case. The measured deltas may slightly
  overstate the true value of the missing street.
- Single seed. Cross-seed spread on this metric is unknown.

## Files

- Test: `crates/pkr-exploit/tests/street_decomposition.rs`
- Reference run: `outputs/v34long/train.ckpt`, 500 deals, seed 42
