# Gadget search H2H - corrected (EV cbv) gadget (2026-10-09)

The gadget-search H2H run on the CORRECTED (EV cbv, proven-safe) gadget,
V5, 20k deals:

  seed 42: diff(A=search - B=blueprint) = +0.6203 chips/deal
           SE 0.1206, t = +5.14

For comparison the BROKEN (BR cbv, unsafe) gadget gave +1.1080 (t=5.68)
on the same seed. The safe gadget wins by less - it is more conservative
(blends toward the blueprint more) - but it is PROVABLY safe and still
clearly positive. seed 43 running.

Shipping read: with the bound (which keeps the raw search whole ~97% of
the time), search H2H is +0.82..+1.18 over 4 seeds; with the (safe)
gadget, +0.62 on seed 42. Either safe config beats the blueprint.
