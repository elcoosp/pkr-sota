# Signature dimensions ladder: V1 -> V3 -> V4 (2026-10-03/04)

LBR (real-game lower bound) at 20k hands, seed 42, off-tree sizes
[0.33, 0.75, 1.5, 3.0], 100M iters each, k=200, same tables.

| arm | added dim | infosets | LBR mbb | SE | d vs prev |
|---|---|---|---|---|---|
| V1 | — | 1.47M | 1115.6 | 109.6 | — |
| V3 | faced bet size | 4.07M | 874.5 | 86.9 | -241 (1.72s) |
| V4 | + pot/SPR class | 6.79M | 770.7 | 88.4 | -104 (0.84s) |

**V1 -> V4 = -344.9 mbb, -30.9%, 2.45 sigma** (vs V1).

## Reading

- Each added dimension lowers real-game exploitability, monotonically.
  V4 vs V1 is significant; V4 vs V3 alone is not (0.84s, one seed).
- The in-sample metric ranks these BACKWARDS (V1 2647 < V3 4940 < V4
  6215) — more infosets = more overfit inflation. LBR is the honest
  ruler; this ladder is report R1+R2 demonstrated end to end.
- Cost: V4 is 4.6x the infosets and ~3x slower to train than V1.

## What this establishes

Adding the *correct coarse* missing information to the infoset key is a
real, repeatable lever on real-game strength (report R2). Two dims so
far; the fix is NOT the full action sequence (31x) nor 5-bit pot class
(41x) — it is the minimal V3/V4 fields.

## Open

- One seed per arm. A V4 second seed would confirm the 104-mbb V4-vs-V3
  step specifically.
- Next dims to try: previous-street aggressor, finer faced-bet buckets.
