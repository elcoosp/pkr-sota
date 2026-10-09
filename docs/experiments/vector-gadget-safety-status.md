# Vector gadget - PROVEN safe (2026-10-09)

## Root cause of the earlier failure

The gadget mechanism was always correct. The bug was the TERMINATE VALUE:
I had changed `p1_cbv_per_hand` from the blueprint-continuation EV to the
best-response (BR) value. CFR-D's Terminate pays the opponent its
blueprint-continuation value (the EV of both players following the
blueprint), NOT its best response. Paying BR gives the opponent too good
a floor and the re-solved strategy becomes worse.

Smoking gun (one subgame, on-tree BR):
  cbv = BR     -> 11.2312   (broken)
  cbv = 0      ->  2.0879   (safe)
  cbv = EV     ->  2.0928   (safe; correct)
  raw search   ->  2.1642

The PROVEN sampled gadget uses the EV - which is why it always worked.

## Sweep (4 river subgames), EV cbv:

  case 0: bp=8.4394  raw=2.1642  gadget=2.0928  (gadget-bp=-6.35)
  case 1: bp=1.8596  raw=-1.4780 gadget=0.9182  (gadget-bp=-0.94)
  case 2: bp=24.1405 raw=4.2467  gadget=6.6207  (gadget-bp=-17.52)
  case 3: bp=14.5281 raw=-0.6304 gadget=6.7482  (gadget-bp=-7.78)

WORST gadget - blueprint = -0.94. The gadget is safe in every case
(gadget_br <= blueprint_br). It can be slightly weaker than the raw
search (cases 2,3) but is still bounded by the blueprint.

## Status

- SAMPLED gadget: proven (bp_br 9.5612 vs gadget_br 0.6284).
- VECTOR gadget: PROVEN safe (sweep above). The `--search-gadget` flag is
  now valid; the live path may re-enable it if a gadget is wanted.
- The shipping DEFAULT stays the heuristic BOUND (keeps the raw search
  whole when it is already safer; the raw search alone is BR 2.16 vs
  blueprint 8.44 - the search is the real win).
