# Suit-isomorphism port plan (for the precompute)

## Motivation
River bucket precompute at `EHS_SAMPLES=100` costs roughly
`C(52,5) * 200 * 100 = 5.2e10` evaluate_hand calls. That is the
dominant cost of `generate_river_buckets`.

## Key insight
The 10-bin EHS histogram for a river board depends on:
- the rank multiset of the 5 cards
- the suit pattern (which cards share suits, and whether a flush is present)

It does NOT depend on which specific suits appear. Under S_4 relabeling
of the suit indices, many boards are equivalent.

## Approach
1. Canonicalize each board: apply the S_4 permutation that
   lexicographically minimizes the tuple.
2. Group boards by their canonical form.
3. Compute the EHS histogram ONCE per canonical form.
4. Copy the bucket assignment back to every member of the orbit.

## Verification gate
The Rust port MUST produce byte-identical `river_buckets.bin` to the
current version on the same board set. Add a `--verify-suit-iso` mode
that:
- runs the precompute normally
- runs the suit-iso-optimized path
- asserts the two output files match byte-for-byte

## Expected speedup
Determined by the probe above. Theoretical upper bound is ~24x (S_4),
realistic is lower because boards with no suit overlap (rainbow) have
small orbits and boards with a single suit class (all same suit) are
already canonical.

## Where to add it
`crates/pkr-abstraction/src/bin/precompute.rs`,
function `generate_river_buckets`, around line 591.

## Cost/benefit
- Rust port: ~2-4h
- Applies only to the river subcommand (turn uses a different feature pipeline)
- Pays off on every subsequent `REBUILD=1` cycle
- Skip if we don't regen again; do it if we're tuning RIVER_BUCKETS or
  RIVER_TIER_SHIFT repeatedly.
