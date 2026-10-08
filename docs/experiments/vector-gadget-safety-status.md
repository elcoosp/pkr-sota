# Vector gadget safety - NOT PROVEN (final, 2026-10-08)

Three fixes attempted, all confirmed applied:
1. chooser = payee (was 1-payee) - CFR-D correct.
2. cbv computed on the VECTOR tree (was the sampled tree).
3. safety measured as P1 BR in the REAL subgame (follow-only),
   excluding the gadget Terminate node.

Result (real V5, river subgame):
  blueprint_br = 8.4394
  gadget_br    = 11.2312   (STILL worse than blueprint)

CONCLusion: the vector gadget does NOT bound exploitability by the
blueprint here. It is UNPROVEN / apparently INCORRECT.

The earlier PROVEN claim (blueprint 8.44 vs gadget 2.18) used a buggy
chooser and is RETRACTED.

## What IS proven

- The SAMPLED gadget (safe_resolve_gadget, pkr-subgame/src/lib.rs):
  bp_br=9.5612 vs gadget_br=0.6284. Self-consistent tree. This is the
  one to ship for a real opponent.
- The vector gadget MECHANISM runs (install/solve/read), both seats.

## Open question (not chased further)

Why the vector gadget fails. Candidate causes: CFR not converged at
80 iters on this tree; the aggregate-root reduction interacting with
the gadget; or a residual cbv mismatch. Worth a focused session, but
NOT worth shipping on. Use the sampled gadget.
