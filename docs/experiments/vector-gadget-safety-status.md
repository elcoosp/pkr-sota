# Vector gadget safety — status (2026-10-07, honest)

## What is proven

- **Sampled** CFR-D gadget (`pkr-subgame::safe_resolve_gadget`): verified
  safe on the real V5 blueprint — `bp_br=9.5612 gadget_br=0.6284`
  (gadget resolves a far-less-exploitable strategy). See
  `gadget-safety-evidence.md`.
- **Vector** gadget (`solve_root_gadgetted`): *mechanism* verified —
  both seats execute, cbv is finite/non-trivial, root distributions
  valid (`gadget_both_seats.rs`, 25 runtime tests).

## What is NOT proven

**The vector gadget's safety theorem is not empirically verified.**

I attempted a cross-validation: transplant the vector gadget's root
strategy into the sampled tree and measure its opponent-BR there.
Result: `bp_br=0.9668 vector_gadget_br=2.1998` — the transplant scored
*worse* than the blueprint.

**This cross-check is INVALID**, so it proves nothing either way:
the vector tree's action set (concrete sizes) and continuation (search)
differ from the sampled tree's (bucketed, blueprint), so the gadget's
guarantee does not transfer between trees. The number is an artifact of
the mismatch, not evidence of unsafety.

## What a real verification needs

The vector solver's **own** BR machinery evaluated against a blueprint
continuation defined on the **same** vector tree — which requires the
blueprint strategy at every vector node, i.e. threading the abstract
infoset history (replicating `history_signature_v3/v4/v5`
byte-for-byte) into `build`. That is the careful, not-yet-done piece.

## Bottom line

- Ship search **with the sampled gadget** for a real opponent — it is
  verified safe.
- The vector gadget is a working *mechanism* but its safety bound is
  **unproven**. Do not rely on it as a guarantee until the infoset
  threading lands and the on-tree BR check passes.
