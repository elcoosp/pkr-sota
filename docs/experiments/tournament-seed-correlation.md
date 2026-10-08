# Tournament seed independence — finding (2026-10-08)

## The issue

`crates/pkr-fuzz/src/tournament.rs::deal_hand(seed)` seeds a
`SmallRng::seed_from_u64(seed)`, and `tournament` calls it with
`base_seed.wrapping_add(i)`. Running the SAME comparison at
`--seed 42`, `43`, `44`, `45` therefore feeds the RNG **adjacent
seeds**. `SmallRng` (xoshiro) produces strongly correlated streams for
adjacent seeds, so the four runs are near-duplicates.

Evidence: seeds 42-45 all report `diff (A - B): +0.8890, t=+4.71`,
with the seat means differing only in the 4th decimal (+0.4304 vs
+0.4305). A genuine independent replicate would vary at the 2nd-3rd
decimal.

## Consequence

The "multi-seed confirmation" (`D1-seed43/44/45`) is NOT an
independent replication. It re-measures almost the same deals.

The single-seed result (seed 42: `+0.8890 chips/deal, t=+4.71` on
20k deals) stands on its own — 20k deals is a large sample — but the
across-seed SE is not a valid independent estimate.

## Fix (future)

Seed the deal RNG from a mixed/hashed seed, not the raw integer:
e.g. `SmallRng::seed_from_u64(splitmix64(base_seed ^ i))`, or use a
`StdRng`/`ChaCha` with the seed. Then re-run seeds 42-45 for a real
replication.
