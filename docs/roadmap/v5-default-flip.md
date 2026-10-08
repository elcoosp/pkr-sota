# V5-default flip - VOID (2026-10-08)

This doc previously recommended making V5 the default signature, based
on the V4/V5 LBR A/B (V5 -2.11 vs V4 821.08).

That recommendation is VOID. The cross-process H2H
(docs/experiments/v4-vs-v5-h2h.md) shows V4 and V5 are EQUAL:

- 5000 deals, seed 42: +0.1212 (t=+0.33)
- 5000 deals, seed 43: -0.2768 (t=-0.71)

The -823 mbb LBR gap is a measurement artifact (LBR cannot see through
V5 aliased postflop key). Four instruments: LBR says V5 better;
in-sample BR, arena, and H2H all say EQUAL.

## Action

- DO NOT flip the default to V5 on strength grounds; there is no
  strength difference.
- The signature choice should be re-decided only by H2H
  (examples/v4v5_h2h.rs), never by LBR alone.
- V5 is not harmful (equal), so keeping either is fine; keep V4 as
  the default (fewer moving parts), unless another reason emerges.
