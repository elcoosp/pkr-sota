# V4 vs V5 - cross-process H2H (2026-10-08)

The first HONEST V4-vs-V5 comparison: two pkr-bot-pipe subprocesses,
one per blueprint (different sig_version, cannot share a process),
driven by crates/pkr-runtime/examples/v4v5_h2h.rs with common random
numbers. A-vs-A verified EXACTLY 0.0000.

## Result (500 deals, seed 42)

diff(A=V4 - B=V5) = +1.2260 chips/deal   SE=1.0092   t=+1.21

V4 edges V5, not significant at 500 deals, but the SIGN contradicts LBR.

## The contradiction

- LBR (20k): V4=821.08, V5=-2.11 -> V5 far better (Delta -823, t~6.3)
- in-sample BR: V4=6147, V5=6117 -> roughly equal
- arena vs scripted: V4=192, V5=203 bb/100 -> V5 slightly better
- cross-process H2H: V4 +1.23 (n.s.) -> NOT V5

LBR says V5 is 823 mbb less exploitable; H2H says V5 does not beat V4
head-to-head. In-sample BR and arena also do NOT show an 823 mbb gap.

## Interpretation

The -2.11 LBR is very likely a MEASUREMENT ARTIFACT: the V5 postflop key
(SPR class + 1-bit aggressor) aliases many states, and the greedy LBR
exploiter fails to find deviations against that aliased strategy - a
spuriously low exploitability that does NOT translate to real strength.
This is the third time LBR has misled (sampled search, then V4/V5).

## Pending

A 5000-deal x 2-seed run is in flight to tighten the H2H.
