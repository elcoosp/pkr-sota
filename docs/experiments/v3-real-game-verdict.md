# V3 (faced-bet bucket): real-game improvement — R2 confirmed (2026-10-03)

**Status:** the first measured real-game improvement. 1.72σ, one seed;
a second seed would confirm.

## The result

100M iterations, seed 42, same tables. LBR (report S2) at 20k hands,
seed 42, off-tree sizes [0.33, 0.75, 1.5, 3.0]:

| arm | infosets | in-sample expl | **LBR mbb/hand** | LBR SE |
|---|---|---|---|---|
| V1 signature | 1.47M | 2647 | **1115.6** | 109.6 |
| V3 (faced-bet bucket) | 4.07M | 4939 | **874.5** | 86.9 |

**Delta = -241.1 mbb, combined SE 139.8 -> 1.72 sigma. -21.6%.**

## Why this matters

The in-sample metric ranked V3 as 2.5x WORSE (4939 vs 2647) — the
opposite of the real-game verdict. That is report finding R1 (the
in-sample BR overfits a larger table harder) demonstrated on a real
decision. The V3 curve *descended* (5459 -> 4939) while V1 *rose*
(2183 -> 2647): a shape signal the absolute numbers hid.

The fix is exactly report R2: add the faced bet-size bucket so the key
can tell a 0.5-pot bet from a 2-pot bet. That is the only change —
`faced_bet_bucket` (3 bits), nothing else (attempts with pot_class /
action-sequence were 31x / 41x non-viable).

## Caveats

- 1.72 sigma, ONE seed. Not conclusive. A second seed (different
  training seed, or an independent LBR seed) is needed before flipping
  the default.
- V3 is 2.5x more infosets and ~3.5x slower to train (8.4k vs 30k it/s).
- LBR itself is a lower bound with its own modeling choices.

## Recommendation

- **Second seed** of the V3 A/B to confirm the ~240 mbb gain.
- If confirmed: make `PKR_SIG_V3=1` the default (it already is
  default-on; keep it), regenerate goldens, and re-baseline the docs.
- This is the first real lever found; re-open the F4 / k=250 abstraction
  "negative" results, which were decided on the biased metric.
