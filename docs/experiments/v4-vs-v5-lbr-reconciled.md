# V4 vs V5 LBR - reconciled (2026-10-08)

At EQUAL (stronger) exploiter strength, samples=2000:

| samples | V4 LBR | V5 LBR | gap |
| 400  | 821    | -2     | 823 |
| 2000 | 996.79 | 182.84 | 814 |

The ~800 mbb V4-vs-V5 LBR gap PERSISTS at the stronger exploiter.
So the gap is NOT a weak-exploiter artifact.

## But H2H says V4 == V5 (10k x4, all flat).

Reconciliation: LBR uses OFF-TREE bet sizes (fracs 0.33/0.75/1.5/3.0)
that NEITHER bot plays in self-play. LBR exploits V4 via off-tree
sizes; in H2H both bots only play IN-TREE sizes, so that exploit is
never exercised. Hence:

- vs off-tree aggression (humans, other bots): V5 is ~800 mbb more
  robust than V4 (real advantage).
- in self-play within the abstraction: V4 == V5 (no advantage).

Both true. Not contradictory. The earlier "artifact" framing was too
strong: the -2.11 was a weak-exploiter reading, but the V4-vs-V5 gap
is real and measures OFF-TREE robustness, which H2H does not test.
