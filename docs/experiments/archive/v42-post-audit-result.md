> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

# v42 — first clean post-audit run

**Date:** 2026-09-30
**Status:** completed, plateau-stopped at 18M.

## The setup

First run with the full audit fixes: F1 (estimator reach), F2
(experiment config defaults), F6 (correct legal-action tree), F7
(2.4 GB RSS via lazy allocation). Verified post-hoc:

- `stats.json` env: `momentum=False`, `avg_power=2.0`,
  `explore_epsilon=0.01`, `dcfr_alpha=1.5`, `rm_plus=True`,
  `f5_sequential=True`.
- Fingerprint: `action_legal_v=1`.
- 30M-iteration budget, plateau-stop 5, promote-gate 3, promote-min-
  sigma 2.

## The curve

| iter | expl_mbb | SE |
|---|---|---|
| 3.0M | **3313.4** | 131.5 |
| 6.0M | 3430.3 | 135.3 |
| 9.0M | 3374.6 | 124.4 |
| 12.0M | 3531.8 | 142.2 |
| 15.0M | 3709.4 | 144.7 |
| 18.0M | 3780.4 | 143.7 |

**Best: 3313.4 mbb @ 3M. Then it climbs monotonically.**

The plateau-stop (5 consecutive non-minimum evals) fired correctly at
18M and saved ~12M iterations of compute. The final SKIP-PROMOTE
line shows the sigma gate working: `improvement -467.00 vs
sigma_needed 287.34` — the reading would have been rejected even
without the plateau detector, because the SE is now honest.

## What this says

1. **The F1 fix eliminated the estimator's false-shrinkage
   behavior.** The 5000-deal SE is stable around 130-145 mbb, not the
   1618 mbb the pre-audit handoff recorded. Every reading is now
   comparable to every other.
2. **The "exploitability rises after 30M" pattern the audit and the
   F5 hypothesis were built around appears much earlier than
   expected — at ~3M.** The v34long-era champion peaked at 27M with
   the OLD estimator, so the pattern was attributed to the sampling
   regime. With an honest estimator it shows up at 3M.
3. **The F5 hypothesis is now testable.** If the floor/avg-site
   flags cause the rise, flipping them will flatten the curve. The
   Kuhn result already says the floor dimension is unlikely to help.

## What this does NOT say

- **The 3313 number is not comparable to the pre-audit 2526 mbb
  "champion."** Different estimator, different legal-action tree.
  Both facts are documented in the invalidation doc.
- **The absolute value is not the point.** The point is the *shape*:
  the curve descends then rises, and the plateau stop catches it.
- **The 3M best may be noise.** SE 131 mbb at 5000 deals. The
  descent from 3430 to 3313 is ~1 SE. But the rise after 3M is
  monotone across 5 readings, which is harder to explain by chance.

## Arena result

`pkr-arena` vs the v42 checkpoint, 5000 hands, seed 42:

    bot bb/100: +210.03
    blueprint hit rate: 99.9%

Consistent with the mid-run reading (+193.6). The bot beats the
scripted bots. That has been true all session.

## Next step

The v42 curve shape is the first clean A/B target. Two things to try:

1. **F5 `avg_at_traverser=false`** on the same config. The theory is
   sounder here than the floor dimension. If the rise flattens, ship
   it as the new default.
2. **Longer plateau-stop check.** The current config stops at 5
   non-minimum evals, which fired at 18M. With the curve turning at
   3M, a *smaller* stop threshold (2 or 3) would have saved more.
   Worth a sweep on the next run.

Both are 30-minute runs. Neither needs the F3 or F4 structural
changes.
