# V5 LBR -2 was a WEAK-EXPLOITER artifact (2026-10-08)

Same V5 checkpoint, same hands, only the LBR exploiter strength varied:

- samples=400  (default): V5 LBR = -2.11
- samples=2000 (5x):     V5 LBR = +182.84  (SE 84.46)

A 5x-sample greedy exploiter finds ~185 mbb of exploit the default
exploiter missed. So V5 is NOT unexploitable - the -2.11 was the
exploiter being too weak to find deviations.

## This closes the loop

- LBR(400):  V4=821, V5=-2  -> looked like 823 mbb gap (ARTIFACT)
- LBR(2000): V5=183         -> V5 is exploitable for ~180+ mbb
- H2H (10k x4): V4 == V5    -> no real gap

CONSISTENT STORY: V4 and V5 are roughly equal strength; the LBR(400)
gap came from V5 aliased postflop key defeating a weak greedy walker.
A stronger exploiter recovers most of the exploitability.

PENDING: V4 LBR at samples=2000 (control) - to compare V4 vs V5 at
equal exploiter strength.
