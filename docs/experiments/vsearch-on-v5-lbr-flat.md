# Vector search on V5: LBR is flat (2026-10-07)

Overnight instrument B: V5 checkpoint, river search, bounded, 20k hands.

| arm | LBR mbb/hand | SE |
|---|---|---|
| V5 blueprint (A) | -2.11 | 98.75 |
| V5 river search, bounded (B) | -122.96 | 100.34 |

Δ = -120.85, combined SE ≈ 141, **|t| ≈ 0.86 — NOT significant.**

Bound diagnostics: `solves=1842 kept_search=1826 blended=16 fell_back=0`.
The bound fired only 0.9% of the time, so it is NOT the reason the
search fails to improve. The reason is that **V5's blueprint is already
essentially unexploitable by LBR** (-2.11 ≈ 0 within noise).

## Why this is coherent, not a contradiction

On the V4 checkpoint (2026-10-06), the blueprint LBR was **821** and the
river search cut it to **444** (-377). There was a lot of exploit to
remove. On V5, the better signature removed most of it, leaving LBR at
~0. Search can only *reduce* exploitability; with none left, it has
nothing to do. The search's value scales with the blueprint's
exploitability, which V5 minimized.

## What LBR being ~0 does and does not mean

- LBR is a *lower bound* from ONE greedy exploiter. -2.11 means this
  exploiter finds no meaningful exploit; it does NOT prove V5 is
  unexploitable.
- The more sensitive instrument is **H2H** (instrument D): does the
  search side win chips against the blueprint side? That runs next.

## Cost note

B took ~15 min vs ~8 min for the unbounded river search on V4 — the
bound adds ~2 opponent-BR walks + a per-hand blueprint hash per solve
(`opp_br_value` twice + `blueprint_root_strategy`), even when the blend
does not fire. For a real-time bot this matters; the
`--search-budget-ms` cap handles it. For measurement it is fine.

## Pre-registered interpretation for instrument D

- If **D H2H > +2·SE**: search still helps on V5 even though LBR is
  flat → LBR was blind to it, ship search.
- If **D H2H flat**: on V5 the search adds nothing measurable →
  the V5 blueprint alone is the deliverable; search matters only on
  weaker checkpoints.
