# CFR-D gadget safety — real evidence (2026-10-07)

## Result

Sampled solver's `gadget_not_more_exploitable_than_blueprint` against
the real V5 checkpoint + v42-post-audit tables:

    bp_br=9.5612  gadget_br=0.6284  (gadget <= bp is SAFE)

bp_br = opponent BR vs the pure blueprint root; gadget_br = vs the
gadget-resolved strategy. 0.63 << 9.56 — the CFR-D construction
resolves a far-less-exploitable strategy on real artifacts.

## Validates
- The CFR-D gadget algorithm (`pkr-subgame::safe_resolve_gadget`).
- Terminate correctly caps the opponent at the blueprint value.

## Does NOT validate
- The vector gadget (`solve_root_gadgetted`): mechanism verified
  (`gadget_both_seats.rs`), theorem not directly measured (needs
  all-nodes blueprint / signature threading).
- Scale: 8-hand ranges.

## Upshot
Real opponent: use the sampled `safe_resolve_gadget` (now tested safe),
or finish the vector all-nodes blueprint.
