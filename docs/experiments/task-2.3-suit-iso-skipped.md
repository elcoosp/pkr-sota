# Task 2.3(a) suit-isomorphism - NOT WORTH IT (measured 2026-10-09)

The plan proposed canonicalizing river boards under S_4 (suit
relabeling) to compute EHS once per orbit. Ran the existing probe
(scripts/analysis/suit_iso_probe.py, 50k sampled boards):

  distinct canonicals: 41340 / 50000
  effective dedup:     1.21x

The theoretical 24x is not realized: most 5-card boards are rainbow or
near-rainbow, whose orbits are tiny (size-1 orbits = 67% of the sample).

A 1.21x precompute speedup does not justify a 2-4h Rust port plus a
byte-identical verification gate, especially since the tables are not
being regenerated right now. The plan itself says "skip if we do not
regenerate again".

DECISION: skip 2.3(a). If RIVER_BUCKETS is retuned later and precompute
becomes the bottleneck, revisit - the probe and the port plan are on disk.
