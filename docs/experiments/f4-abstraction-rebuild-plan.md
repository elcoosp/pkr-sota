# F4 — Abstraction rebuild: measured cost and plan

**Date:** 2026-09-30
**Status:** infrastructure ready, rebuild not launched.
**Supersedes:** the audit's estimate. Replaces it with wall-time measured
on the actual hardware.

## What's ready

`crates/pkr-abstraction/src/potential.rs` provides:

    ehs_and_potential(hole, board, ev, inner) -> (mean, potential)

- `mean`: equity vs a random hand averaged over next-street cards.
- `potential`: standard deviation of that equity across next cards,
  with per-card sampling variance subtracted out. This is the real
  second feature that `EHS²` was supposed to be.

Deterministic given (hole, board). No allocations. 4 unit tests pass.

## Measured cost

`potential::tests::timing_sample`, 1000 flop hands, `inner=20`:

    4.44 s wall, 4439 us/hand.

Extrapolating to the real rebuild (no parallelism, no isomorphism):

| table | boards | hands | entries | single-core wall |
|---|---|---|---|---|
| flop  | 22,100 | 1,176 | 25,989,600 | ~32 h |
| turn  | 270,725 | 1,128 | 305,377,800 | ~15 days |

With 8 threads and suit isomorphism (flop / turn reduce by ~20×
because all permutations of a 3- or 4-card board's suits are
equivalent):

| table | parallel + isomorphic wall |
|---|---|
| flop  | ~2 h |
| turn  | ~2.5 days |

That's the minimum realistic rebuild. Both are background jobs.

## Cost reduction options

1. **Suit isomorphism.** The audit's ~20× estimate. Requires an
   isomorphism class map for flop and turn boards. The `all7`
   subcommand is the natural place to hang this. Estimated effort:
   1 day, high payoff (turn goes from 15 days to < 2 days).
2. **Reduce `inner`.** At `inner=10` the wall halves; below ~8 the
   potential estimate is noisier than the underlying signal the
   audit measured (0.0014 oracle std). Not recommended.
3. **Reduce target tables.** The audit's within-bucket-std gate
   (below 0.01) is the acceptance criterion. Compute the flop table
   first, measure, then decide whether the turn rebuild is worth
   2.5 days.

## Acceptance gate

Before this replaces the current tables:

1. Recompute the audit's within-bucket-std measurement against the
   new tables. Target: < 0.01 (currently 0.044).
2. Run `pkr_fuzz::tournament` with the new abstraction vs the old.
   The new one must win (mean_diff > 0 with t > 2 at 100k paired
   deals), or at minimum not lose.
3. Update `AbstractionFingerprint::from_constants` — the fingerprint
   is already gated on `preflop_k`, `sig_version` and the sizing
   constants, but the *feature space* isn't captured. Add a
   `centroid_feature_v: u8` field (0 = EHS/EHS², 1 = mean/potential)
   so a checkpoint trained on one feature space refuses to load on
   the other. That's a separate small commit before the rebuild
   launches.

## Not yet implemented

- Suit isomorphism for flop and turn. The `all7` subcommand may
  already have infrastructure; that's an inspection task before
  committing to the 2h/2.5d estimate.
- Per-street k-means fit on (mean, potential). The current
  `generate_centroids` runs once and reuses preflop centroids for
  every street. The rebuild must fit flop and turn centroids from
  flop and turn samples, not from preflop.
- The `flop-rich` / `turn-rich` subcommands in the precompute binary
  are 10D (EHS, EHS², rank, suit, gap, 4 board features) and don't
  apply. The new rebuild wants a *2D* (mean, potential) space, so a
  new subcommand or a mode switch is needed.

## Recommendation

Do not launch the full rebuild this session. Two things must happen
first:

1. Suit isomorphism for flop/turn (1 day of work, 20× payoff).
2. `centroid_feature_v` fingerprint field (2h) so old/new
   checkpoints can't be mixed up.

Both are smaller than the rebuild itself. Then the flop rebuild is a
2h background job and the turn a 2.5d one.

---

## Rebuild completed (2026-10-01)

The flop table rebuild finished in 8 minutes, not the ~2h the earlier
timing estimate predicted. Measured: `centroids-potential 500 200`
took 6 seconds; `abs-potential` on 26M entries took 8m42s.

What changed between old and new:

| | old (EHS, EHS²) | new (mean, potential) |
|---|---|---|
| bytes differing | — | 25,892,395 / 25,989,600 (99.6%) |
| distinct buckets | 193 | 200 |
| bucket entropy | 6.687 bits | 6.557 bits |

**99.6% of bucket assignments changed.** The potential feature is
doing real work; it's not a reshuffle of the same information.

Centroid coordinate stats:
- mean EHS = 0.2854, sd = 0.2276
- mean potential = 0.2626, sd = 0.0888

The potential dimension carries ~40% of the EHS dimension's spread —
meaningful but not equal. A more aggressive build could weight it up,
but that's a follow-up.

### v45 training run

`outputs/v45-potential/` runs the standard 30M config with the new
flop table. Same as v42 otherwise, so the A/B is clean.

### Known issue: fingerprint doesn't reflect the new feature space

`centroid_feature_v` should be `1` for v45, but the trainer writes the
default `0` because it doesn't know what feature space the tables
describe. The A/B itself is valid (v45 trains and evaluates against
its own tables consistently), but a future load of v45's checkpoint
against v34long's tables would NOT fail the fingerprint — both claim
`centroid_feature_v=0`.

The fix is a `--centroid-feature-v` CLI flag or deriving the value
from a manifest file next to `centroids.bin`. Tracked as a follow-up;
not worth restarting v45 for.
