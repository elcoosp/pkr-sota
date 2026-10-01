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

---

## Follow-up: `centroid_feature_v` now env-driven

`AbstractionFingerprint::from_constants` reads `PKR_CENTROID_FEATURE_V`
(0 or 1, default 0). A launcher for an F4 run exports it once and every
component — trainer, arena, tournament — builds the same fingerprint.

Values:
- `0` = legacy (EHS, EHS²). Every pre-F4 checkpoint.
- `1` = (mean, potential). The v45 tables.

Update the v45 launcher to export it before launching. Fingerprint
mismatches now correctly reject cross-feature-space loads.

---

## v45 first eval (3M)

| | v42 (legacy EHS/EHS²) | v45 (mean/potential) |
|---|---|---|
| 3M reading | 3313.4 mbb | **3392.3 mbb** |
| SE | 131.5 | 142.1 |

Delta +78.9 mbb, well within the ~137 pooled SE. One eval, not
conclusive. The curve shape over the next 4-5 evals is what matters:
if v45 turns up at 3-6M like v42 does, the new feature doesn't change
the fundamental pattern.

Compare against the arena once v45 has a loadable checkpoint and a
full run.

---

## RESULT (2026-10-01): F4 is EQUIVALENT — keep legacy

v45 finished at 18.0M iterations (plateau-stop 5).

| iter | v42 (legacy) | v45 (potential) |
|---|---|---|
| 3.0M | **3313.4** | 3392.3 |
| 6.0M | 3430.3 | **3302.3** |
| 9.0M | 3374.6 | 3501.0 |
| 12.0M | 3531.8 | 3593.7 |
| 15.0M | 3709.4 | 3499.6 |
| 18.0M | 3780.4 | 3612.5 |
| **best** | **3313.4** | **3302.3** |

delta of bests = -11.0 mbb, combined SE ~192. Per the decision rule
(within +/-260 = equivalent): **keep the legacy (EHS, EHS-squared)
centroid feature. Do NOT launch the 2.5-day turn rebuild.**

### Both curves turn up in the same place

Best early (3-6M), then rising exploitability — the same shape as
v42-vs-v43. F4 does not fix the turn-up. With F5 neutral on the
averaging site and RM+ helping on the floor (Kuhn), the turn-up is
still unexplained. The untested `avg_power` dimension is the leading
remaining candidate; see the correction note in `f5-grid.md`.

### Two operational findings

1. **The promote gate froze v45 at 3M.** The 6M reading (3302.3) was
   never promoted: the gate needs `2*SE ~ 280` mbb of improvement, but
   it was only 90 mbb better than the 3M best. So `blueprint.bin` on
   disk is the 3M model (3392.3), ~90 mbb WORSE than v42's 3313.4.
   Best-reading comparison says "equivalent"; best-artifact comparison
   says v42 wins. Revisit `--promote-min-sigma 2`: it discards real
   100-200 mbb improvements. The 6M eval point was not a checkpoint
   boundary, so it cannot be recovered retroactively.

2. **The watcher's arena step failed on the fingerprint guard.** It ran
   `arena` with `PKR_CENTROID_FEATURE_V` unset (=0) against a
   checkpoint trained with =1. The guard correctly refused to load —
   but the watcher must export the var. This is the same class of
   footgun the `resolve_tables_dir` warning covers for `tournament`.

### Promote gate discards the best model (recurring)

`--promote-min-sigma 2` rejects any eval whose improvement over the
current best is below `2 * SE` (~256 mbb at SE~128). When that happens,
`best_expl_mbb` is NOT updated and the artifact is NOT re-exported. So
the "best reading" and the saved blueprint can diverge:

- v45 best reading: 3302.3 @ 6M. Promoted artifact: 3392.3 @ 3M.
  The 6M point was 90 mbb better but under the sigma gate, so the
  better model was never saved.
- v46 will hit the same trap: 3M promotes at 3231.8, so nothing above
  3231.8 - 256 = 2976 can ever promote.

The gate is defensible (winner's-curse protection: a sub-sigma win may
be luck), but for A/B decisions we compare READINGS, not artifacts, so
the decision is unaffected. It only bites when SHIPPING the best model.
A `--save-best-reading` flag (export on every new minimum, regardless
of the gate) is the clean fix; deferred until v46 finishes so its
binary stays valid.
