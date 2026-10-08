# Shipped safe config: the BOUND, not the vector gadget (2026-10-08)

The vector gadget is UNPROVEN (worse than blueprint on-tree). So the
gadget-search H2H runs (GAD-seed42 = +1.108, GAD-seed43 running) measure
a strategy whose SAFETY is not established. They show the search helps
head-to-head but do NOT validate the shipping config.

The SHIPPED safe search config is the heuristic BOUND:

- Live path (session.rs) now uses solve_root_bounded (gadget disabled).
- Bound safety PROVEN by test bounded_root_never_worse_than_blueprint
  (final_br 0.0205 <= blueprint_br 0.0245).
- Bound H2H positive across 4 independent seeds:
  +1.1763 / +0.8211 / +0.9722 / +1.1625 (all t>4).

So: ship --search-bound (safe + wins). The vector gadget stays parked.
