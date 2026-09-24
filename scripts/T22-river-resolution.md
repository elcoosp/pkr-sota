# T2.2 — river hand-resolution increase

## Current state vs stale audit text

The audit's docs/PKR_AUDIT_AND_FIX_PLAN.md section 8 describes T2.2 as
hand_rank >> 6 -> >> 3. That description is stale: it predates the F6 fix
that moved the river hash from >> 6 (raw-bit ranks, ~130K tiers, wrong)
to >> 15 (~287 tiers, correct). Current code:

    crates/pkr-abstraction/src/lib.rs:422
    let hand_bucket = hand_rank >> 15;  // ~0..=287, monotone

## What T2.2 should actually be on this code

Two levers, both [HASH] (invalidate artifacts, require retrain):

### Lever A — finer hand resolution

    shift    tiers     comment
    >> 15    ~287      current
    >> 14    ~575
    >> 13    ~1150     4x finer — recommended
    >> 12    ~2296

### Lever B — coarser board buckets

    RIVER_BUCKETS=200   current default (scripts/run-config.sh:23)
    RIVER_BUCKETS=128   recommended T2.2 value

Net river keyspace at (>> 13, 128): 4 * 0.64 = ~2.5x current.

## Files to change

1. crates/pkr-abstraction/src/lib.rs:422
   - hand_rank >> 15  ->  hand_rank >> 13
   - Also update monotonicity tests (~664, ~791) to expect >> 13.

2. scripts/run-config.sh:23
   - RIVER_BUCKETS default 200 -> 128

## Cost

- Precompute: hand_ranks + centroids unchanged. Only river_buckets.bin
  and turn_abstraction.bin regenerate. ~5-10 min with PKR_EVALUATOR=fast7.
- Retrain: same as v25final, ~1.5-2h at 200M iters.
- Disk: ~500 MB.

## Success criterion

At 100M+ iters, matched wall-clock, expl_mbb lower than v25final's at
the same iteration by more than 2 sigma. If not, ceiling is elsewhere.

## Alternatives if T2.2 does not move the needle

1. PKR_EXPLORE_EPSILON sweep (0.005 vs 0.01 vs 0.02). Known big lever:
   v23 -> v23a3 already showed -2250 mbb at 20M for 0.05 -> 0.01.
2. SIG_V2_STREET_MONEY=true — bet-size-aware infosets.
3. Turn k=400 — more turn resolution.
