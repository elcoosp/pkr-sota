# V4 vs V5 - cross-process H2H (2026-10-08)

First HONEST V4-vs-V5 comparison: two pkr-bot-pipe subprocesses (one
per blueprint, different sig_version so they cannot share a process),
driven by examples/v4v5_h2h.rs with common random numbers. A-vs-A
verified EXACTLY 0.0000.

## Results (CRN, independent seeds)

- 500  deals, seed 42: +1.2260  SE=1.0092  t=+1.21
- 5000 deals, seed 42: +0.1212  SE=0.3716  t=+0.33
- 5000 deals, seed 43: -0.2768  SE=0.3872  t=-0.71

## Verdict: V4 and V5 are EQUAL head-to-head.

At 5000 deals both seeds are flat (|t| < 1). The sign flips between
seeds. There is NO real strength difference between V4 and V5.

## The contradiction with LBR

- LBR (20k): V4=821.08, V5=-2.11 -> claims V5 is 823 mbb better (t~6.3)
- in-sample BR: 6147 vs 6117 -> equal
- arena vs scripted: 192 vs 203 bb/100 -> within noise
- cross-process H2H (5k x2): flat -> EQUAL

Four instruments, only LBR shows a gap; the three independent ones say
EQUAL. The -2.11 LBR is a MEASUREMENT ARTIFACT.

## Why LBR is fooled by V5

V5 postflop key = SPR class (2 bits) + prev-street-aggressor (1 bit),
aliasing many states to one strategy. LBR is a single greedy walker;
against an aliased/smooth strategy it finds no deviation and reports a
spuriously low exploitability. This is the THIRD LBR failure
(sampled search, then V4/V5).

## Consequence

NEITHER V4 NOR V5 is shown to be competitive. The honest number is the
H2H (equal) and the LBR floor (~800 mbb for V4). Do not ship a
"competitive bot" claim on the V5 LBR.
