# V4 vs V5 - off-tree resolution + valid H2H (2026-10-08)

## Off-tree hypothesis: PARTIALLY confirmed

LBR at samples=2000, in-tree-only vs with off-tree sizes:

| config            | V4 LBR  | V5 LBR | gap |
| in-tree only      | 226.92  | 28.71  | 198 |
| with off-tree     | 996.79  | 182.84 | 814 |

Off-tree sizes account for ~616 of the 814 mbb gap (76%). A ~198 mbb
in-tree gap REMAINS.

## H2H (FIXED bot, valid): V4 marginally ahead, not significant

10k deals x 4 seeds (CRN, A-vs-A == 0.0000):
  +0.2685 (t=+0.94) / +0.0763 (+0.27) / +0.1604 (+0.57) / +0.1016 (+0.37)
  mean +0.152, all |t| < 1 -> EQUAL, V4 consistently marginally ahead.

(The earlier V4==V5 run used a buggy bot - seat-1 hole was not passed;
see v4v5-h2h-invalid-buggy-bot.md. This is the corrected run.)

## Reconciliation

V5 is genuinely LESS LBR-exploitable than V4 (both in-tree ~198 and
off-tree ~814), yet does NOT beat V4 in self-play H2H. LBR exploitability
and self-play strength are different axes: LBR finds deviations (sizes,
lines) that neither bot exercises against a copy of itself.

## Practical read

- vs OFF-TREE aggression (humans / other bots): V5 is the more robust
  choice (~800 mbb less exploitable).
- in self-play / same-abstraction opponents: V4 and V5 are equal.
- Neither is shown competitive (V4 in-tree LBR floor ~227 mbb).
