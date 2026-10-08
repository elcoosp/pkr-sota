# Session 2026-10-08 - verified findings (consolidated)

## Search: WORKS

- Vector river search, V5, H2H vs blueprint, 4 independent seeds (CRN,
  fixed seed spacing): +1.1763 / +0.8211 / +0.9722 / +1.1625 (all t>4).
- Gadget-bounded search, V5, H2H seed 42: +1.1080 (SE 0.195, t=+5.68).
- On V4 (earlier): LBR 821->444, H2H +1.254 (t=6.25).
- Arena (scripted, 300 hands): blueprint 69.25 vs +search 102.27 bb/100.

## CFR-D gadget: SAMPLED proven, VECTOR NOT

- SAMPLED gadget (safe_resolve_gadget): bp_br=9.5612 vs gadget_br=0.6284
  on real V5. Self-consistent tree. THIS is the one to ship.
- VECTOR gadget: NOT proven. Earlier "blueprint 8.44 vs gadget 2.18" used
  a BUGGY chooser (1-payee). After 3 fixes (chooser=payee; cbv on the
  vector tree; follow-only BR measure) it is STILL worse than blueprint
  (gadget_br 11.2-11.4 vs blueprint_br 8.44). Not a convergence issue
  (400 iters also fails). See vector-gadget-safety-status.md.
- Infoset-signature threading: byte-verified (1356 nodes, 0 mismatches).

## V4 vs V5: EQUAL in-abstraction; LBR gap is real but off-tree (hypothesis)

- H2H 10k x4 seeds: all flat, |t|<1 -> V4 == V5 in self-play.
- LBR at equal strength (samples=2000): V4=996.79 vs V5=182.84 -> gap 814.
- Hypothesis: gap is OFF-TREE robustness (LBR uses fracs 0.33/0.75/1.5/3.0;
  H2H uses only in-tree bucket sizes). Test: lbr_eval --no-offtree.

## Still true: no competitive-bot claim

V4 LBR floor ~800-1000 mbb. Nothing shows world-class strength.

## Tooling

- SearchHandle trait; --search-vector/-bound/-gadget on lbr_eval/tournament.
- examples/v4v5_h2h.rs: cross-process H2H with CRN (A-vs-A == 0.0000).
- pkr-bot-pipe: real actions + board advancement.
- tournament seed-spacing fixed; lbr_eval --no-offtree.
