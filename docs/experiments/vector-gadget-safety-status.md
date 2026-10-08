# Vector gadget safety - UNPROVEN (corrected 2026-10-08)

The earlier "PROVEN" (blueprint_br=8.44 vs gadget_br=2.18) used a BUGGY
gadget: install_gadget set chooser = 1 - payee, i.e. the ACTING player
was given the terminate option, which is meaningless. Fixed to
chooser = payee (CFR-D: the chooser is the one paid their cbv).

With the corrected gadget, the on-tree safety test FAILS:
  blueprint_br = 8.4394
  gadget_br    = 11.8048   (WORSE than blueprint)

So the vector gadget is NOT proven safe. The earlier claim is retracted.

## Root-cause hypothesis (not yet fixed)

p1_cbv_per_hand computes the terminate value on the SAMPLED tree
(pkr-subgame::POCConfig / lib.rs Solver), but the gadget is installed
on the VECTOR tree (vector.rs Solver). Different action buckets /
continuations => the sampled-tree cbv is not P1 BR value in the
vector tree => the terminate floor is wrong => the gadget does not
bound. This is the same cross-tree problem that invalidated the
earlier transplant test.

## What is still true

- Sampled gadget (safe_resolve_gadget): proven safe (bp_br=9.5612 vs
  gadget_br=0.6284) - that is a DIFFERENT, self-consistent tree.
- The vector gadget MECHANISM runs (install/solve/read), both seats;
  its SAFETY is unproven.
