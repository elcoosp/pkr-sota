# V4 vs V5 - cross-process H2H (2026-10-08) - FINAL

First HONEST V4-vs-V5 comparison: two pkr-bot-pipe subprocesses (one
per blueprint, different sig_version, cannot share a process), driven
by examples/v4v5_h2h.rs with common random numbers. A-vs-A verified
EXACTLY 0.0000.

## Results (CRN, independent seeds)

500 deals:  seed 42: +1.2260  (t=+1.21)
5000 deals: seed 42: +0.1212  (t=+0.33)
5000 deals: seed 43: -0.2768  (t=-0.71)
10000 deals: seed 42: -0.1617 (t=-0.60)
10000 deals: seed 43: -0.0439 (t=-0.16)
10000 deals: seed 44: +0.0367 (t=+0.14)
10000 deals: seed 45: -0.2378 (t=-0.90)

## Verdict: V4 == V5

At 10k deals (SE ~0.27) all four seeds are flat, signs mixed, |t| < 1.
There is NO strength difference between V4 and V5.

## Contradiction with LBR

LBR: V4=821, V5=-2 (claims 823 mbb gap, t~6.3).
in-sample BR: 6147 vs 6117 (equal).
arena: 192 vs 203 bb/100 (equal).
H2H (10k x4): equal.
Only LBR shows a gap -> the -2.11 LBR is a MEASUREMENT ARTIFACT.

## Why

V5 postflop key (SPR class + 1-bit aggressor) aliases many states to
one strategy; a single greedy LBR walker finds no deviation against an
aliased/smooth strategy and reports spuriously low exploitability.
Third LBR failure (sampled search, then V4/V5).

## Consequence

NEITHER V4 NOR V5 is shown competitive. Honest numbers: H2H equal,
V4 LBR floor ~800 mbb. No competitive-bot claim.
