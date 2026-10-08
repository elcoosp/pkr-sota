# V5 LBR seed stability (2026-10-08)

| seed | V5 LBR mbb/hand | SE |
|---|---|---|
| 42 | -2.11 | 98.75 |
| 43 | -2.07 | 98.75 |

The ~-2 mbb reading is STABLE across seeds — not seed noise.

## Interpretation (unchanged conclusion, sharper)

A stable LBR does NOT mean the bot is strong; it means this ONE greedy
exploiter consistently finds no deviation against V5. LBR is the
instrument that already mis-scored the sampled search (handoff §2).

The decisive robustness test is a STRONGER exploiter (more samples,
different off-tree sizes): if a 5x-sample exploiter still finds ~0,
V5 is genuinely hard for greedy exploiters; if it finds hundreds, the
-2 is an artifact of LBR being too weak.

In-sample BR (V4 6147 vs V5 6117) and arena (192 vs 203 bb/100) do NOT
corroborate an 823 mbb gap — so the honest read is still that the V4/V5
LBR gap overstates the strength difference.
