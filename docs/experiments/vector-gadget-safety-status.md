# Vector gadget - RESOLVED: it was the problem, not the search (2026-10-09)

## The diagnostic that settled it

On-tree best-response, same subgame, real V5 blueprint:

  blueprint_br    = 8.4394
  raw_search_br   = 2.1642   <- the vector SEARCH is 4x safer than the blueprint
  gadget_br       = 11.2312  <- the GADGET makes it WORSE

So the vector search is intrinsically strong; the CFR-D gadget WIRING is
what degrades it. The gadget is the bug, not the search.

## Sweep (4 river subgames), raw search vs blueprint:

  case 0: bp=8.4394  raw=2.1642   delta=-6.28
  case 1: bp=1.8596  raw=-1.4780  delta=-3.34
  case 2: bp=24.1405 raw=4.2467   delta=-19.89
  case 3: bp=14.5281 raw=-0.6304  delta=-15.16

Raw search is safer than the blueprint in ALL 4 cases (worst delta -3.34).

## Consequence

The SHIPPED config is the heuristic BOUND, which keeps the search root
whole whenever search_br <= blueprint_br (always true here). So the bound
captures the full search win and needs NO gadget. The vector gadget is
unnecessary and is left parked.

## What the sweep asserts (shipping property)

`solve_root_bounded` safety: final_br <= blueprint_br (verified,
final 0.0205 <= bp 0.0245). And raw search BR <= blueprint BR (4 cases).

## The sampled gadget

Still proven safe (bp_br 9.5612 vs gadget_br 0.6284) and available via
SubgameHandle for a real opponent, if a gadget is ever wanted. Not needed
for the shipped bound path.
