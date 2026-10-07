# CFR-D gadget for the vector solver — design (not yet implemented)

**Status:** Stage 1 (Const node) IMPLEMENTED + tested — see
`cfr-d-gadget-status.md`. Stages 2-4 specified. The shipped safety bound
(`docs/experiments/vsearch-safety-bound.md`) is the heuristic root
blend. This specifies the theorem-backed replacement.

## Why the heuristic is not enough

The root blend bounds the opponent's best response against the TRACKED
ranges (in-sample). An opponent playing off-distribution — one the
RangeTracker did not anticipate — is not covered. The CFR-D
reach-subgame gadget (Burch, Johanson, Bowling 2014) bounds P0's
exploitability by the blueprint's REGARDLESS of the opponent's range.

## The gadget

Wrap the subgame root in one opponent (P1) decision node:

- **Terminate** — pay P1 its counterfactual-best-response value against
  the blueprint, per hand, `cbv[h]`.
- **Follow** — enter the real subgame root.

P1's reach at Follow is its tracked range. Because Terminate caps P1 at
the blueprint's value, P1 cannot gain more than the blueprint allows, so
the resolved P0 strategy is safe independent of the ranges used.

## What the vector solver needs

1. `Node::Const { vals: Vec<f32> }` — terminal paying `vals[h]` to P0.
   Add arms to `cfr` and `br`.
2. `blueprint_strategy_all_nodes` — blueprint strategy at EVERY node
   (not just root). The sampled solver's `build_blueprint_strategy` is
   the template; map actions via `action_bucket`.
3. `p1_cbv_vs_blueprint(bp)` — P1's counterfactual-BR value against the
   fixed blueprint, one tree walk, `Vec<f32>` per hand.
4. Gadget construction: root becomes a P1 `ActNode`
   `[Terminate→Const{cbv}, Follow→real_root]`; solve; read P0's strategy
   at the real (Follow) root.
5. `solve_root_gadgetted(...)`.

## Cost

One extra tree walk (`cbv`) + the solve. Cheaper than the heuristic's
up-to-12 binary-search walks. Compelling reason to implement.

## Acceptance

Adversarial-range test: gadget-solve exploitability against a
best-responding opponent ≤ the blueprint's. Port the sampled solver's
`#[ignore]` test `gadget_not_more_exploitable_than_blueprint`.

## Coexistence

`--search-bound` (heuristic, fast) stays. Add `--search-gadget` (CFR-D)
for real external opponents.

## Effort

~half a day. `Const` node + arms (~1h), all-nodes blueprint fill (~2h),
cbv walk (~1h), gadget + test (~2h). No new deps.
