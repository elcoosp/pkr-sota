# Vector gadget safety — PROVEN (2026-10-08)

## Result

On-tree measurement, real V5 blueprint (outputs/ab-v5/train.ckpt +
outputs/v42-post-audit), test `vector_gadget_safe_on_own_tree`:

    blueprint_br=8.4394  gadget_br=2.1774

- blueprint_br: P1 best response vs P0 playing the BLUEPRINT at every
  node of the vector tree.
- gadget_br: P1 best response vs P0 playing the SOLVED search including
  the gadget node.

The gadget-resolved strategy is about 4x less exploitable (2.18 vs
8.44), same tree, same BR procedure. The vector gadget is safe.

## Why this is valid (unlike the earlier transplant)

Both solvers are vector Solvers over the identical Spec: same root,
ranges, action set, tree shape. The BR walk `solved_exploitability_p1`
runs on that same tree. So this compares a strategy against the
blueprint WITHIN one tree model. The invalid cross-tree transplant
(br_vs_custom_root + vector_gadget_br_le_blueprint) is retired.

## Enabling work

1. Infoset-signature threading (commit 28ef638): ActNode stores
   sig/sig_len from a parallel GameState walk; verified byte-exact
   against GameState::infoset_signature_into over 1356 nodes, 0
   mismatches.
2. p1_cbv_per_hand uses the BEST-RESPONSE value (br_per_deal), not the
   both-blueprint EV. CFR-D Terminate pays the opponent counterfactual
   best response; the EV was too small and the gadget did not bound.
3. solved_exploitability_p1: BR walk where P0 follows its solved
   average strategy and P1 best-responds.

## Caveat

Single river subgame, ~8-hand effective ranges, 80 iterations. Not a
full 1326-hand sweep, not turn/flop. Construction and safety
demonstrated; scale-up is a follow-up.
