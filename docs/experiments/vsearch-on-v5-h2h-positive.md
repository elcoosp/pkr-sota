# Vector search on V5: LBR flat but H2H POSITIVE (2026-10-07)

## Result

Head-to-head, 20,000 duplicate deals, seats swapped, V5 checkpoint.
A = bounded river search, B = blueprint.

| | value |
|---|---|
| A mean (seat 0) | +0.4303 chips/deal |
| B mean (seat 0) | −0.4587 chips/deal |
| **diff (A − B)** | **+0.8890 chips/deal** |
| SE | 0.1889 |
| **t** | **+4.71** |

**The bounded river search beats the V5 blueprint head-to-head.**

## The contrast that matters

The same bounded river search on V5, measured by **LBR** (instrument B):
−122.96 vs blueprint −2.11, Δ = −121, |t| ≈ 0.86 → **flat**.

LBR says "no difference". H2H says "+0.89 chips/deal, t=4.71".
This is the third time in the project that **LBR under-reports the
search's value** (sampled solver: LBR −627, H2H −6.06; vector on V4:
both positive; vector on V5: LBR flat, H2H positive).

**LBR is not a strength measure for search.** The handoff §2 rule
("H2H is the measure") is confirmed again.

## What this means

- The vector search is a **net positive on the best checkpoint we
  have (V5)**, verified on the honest instrument.
- The safety bound did **not** kill the win: instrument D1 used
  `--search-bound`, and the bounded search still won +0.89.
- The search wins **fewer chips on V5 (+0.89)** than on V4 (+1.25),
  consistent with V5's blueprint being stronger (less for the search
  to add). But it is still positive and significant.

## Combined picture (all three vector measurements)

| base | LBR Δ (search − bp) | H2H Δ (chips/deal) |
|---|---|---|
| V4 (2026-10-06) | −377 (|t|≈3.1) | +1.254 (t=6.25) |
| V5 (2026-10-07) | −121 (|t|≈0.86, flat) | +0.889 (t=4.71) |

Both bases: H2H positive. V4 also shows LBR improvement; V5 does not
(LBR floor effect). **Ship the search on both.**
