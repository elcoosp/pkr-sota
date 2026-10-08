# Gadget-bounded search H2H vs blueprint, V5 (2026-10-08)

The SHIPPABLE config: --search-gadget (CFR-D theorem-backed bound),
not the heuristic --search-bound. 20k deals, seed 42:

  diff (A=search - B=blueprint) = +1.1080 chips/deal
  SE = 0.1950
  t  = +5.68

The theorem-backed gadget search beats the blueprint head-to-head by
about the same margin as the heuristic bound (+0.89 to +1.18 across
runs). So the safe config is also the winning config. Seed 43 running.
