# HANDOFF - 2026-10-08 (search bound + gadget + V4/V5 verdict)

Continues HANDOFF_2026-10-07_session4.md.

## THE HEADLINE: V5 LBR was an artifact

The overnight result "V5 LBR = -2.11 mbb" is REFUTED. A cross-process
H2H (the first honest V4-vs-V5 comparison) shows V4 == V5:

- 5000 deals seed 42: +0.1212 (t=+0.33)
- 5000 deals seed 43: -0.2768 (t=-0.71)

Four instruments: LBR says V5 far better (-823 mbb); in-sample BR
(6147 vs 6117), arena (192 vs 203), and H2H all say EQUAL. LBR is
fooled by V5 aliased postflop key. Third LBR failure. See
docs/experiments/v4-vs-v5-h2h.md. The v5-default-flip is VOID.

## What DID hold up: the vector search

Bounded river search beats the blueprint H2H on V5, 4 independent
seeds (CRN + fixed seed spacing): +1.1763 / +0.8211 / +0.9722 /
+1.1625 (all t>4). docs/experiments/search-h2h-replication.md.

## CFR-D gadget

- Vector gadget safety PROVEN on-tree: blueprint_br=8.4394 vs
  gadget_br=2.1774; 4-case sweep all gadget-below-blueprint (worst
  delta -3.33). Required infoset-signature threading (byte-verified,
  1356 nodes, 0 mismatches).
- Sampled gadget terminate value fixed from EV to BR (br_per_deal);
  re-verified bp_br=9.5612 gadget_br=0.6284.

## Tooling built

- SearchHandle trait (sampled + vector); --search-vector/-bound/-gadget
  on lbr_eval/tournament; arena search wired.
- examples/v4v5_h2h.rs: cross-process H2H with CRN (A-vs-A == 0.0000).
- pkr-bot-pipe emits concrete actions + advances the board.
- tournament seed-spacing fixed (was shifted-by-one overlap).

## Running at handoff

- /tmp/v4v5c.sh: V4-vs-V5 10k deals x 4 seeds (tightening the verdict).
- /tmp/lbrhi.sh: V5 LBR samples=2000 (stronger-exploiter robustness).

## Next

1. Read /tmp/v4v5c-result.txt - confirm V4==V5 across 4 seeds.
2. Read /tmp/lbrhi-result.txt - if the 5x-sample LBR also finds ~0,
   V5 is genuinely hard for greedy exploiters; if it finds hundreds,
   even the LBR floor was too weak.
3. Neither V4 nor V5 is shown competitive - the LBR floor for V4 is
   ~800 mbb. Do not ship a competitive-bot claim.
