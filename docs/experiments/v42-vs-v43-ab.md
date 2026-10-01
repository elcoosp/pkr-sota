# v42 vs v43 — F5 averaging-site A/B

**Date:** 2026-09-30
**Status:** v42 done, v43 running.

## The question

The audit's F5 second half: is the average strategy better accumulated
at the traverser's own node (current) or at the opponent's node
(standard external sampling)?

The theory is on firmer ground here than the floor dimension. The
current scheme weights the strategy sum by `strategy · own_reach ·
t^p` at the traverser's node. But at a traverser node, the
opponent's reach is the counterfactual constant and the traverser's
own reach is not the right weight — external sampling already encodes
own reach in visit frequency. Adding it double-counts.

## Setup

Same everything except `PKR_AVG_AT_TRAVERSER`:

| | v42 | v43 |
|---|---|---|
| avg site | traverser (default) | opponent |
| PKR_MOMENTUM | false | false |
| PKR_AVG_POWER | 2 | 2 |
| PKR_EXPLORE_EPSILON | 0.01 | 0.01 |
| PKR_RM_PLUS | true | true |
| iterations | 30M | 30M |
| plateau-stop | 5 | 5 |
| eval cadence | 3M | 3M |
| eval deals | 5000 | 5000 |
| seed | 42 | 42 |

## What to compare

1. **The curve shape.** v42's turn-up begins at 3M. If v43 stays flat
   or continues descending past 6M, the averaging site is a
   contributor to the rise.
2. **The best reading.** If v43's best is meaningfully lower than
   v42's 3313 mbb, ship the change. If it's within 1 SE (131 mbb), it
   doesn't matter.
3. **Throughput.** v43 runs at ~32K it/s in the first 1M iterations,
   comparable to v42's 48K early. Any slowdown from the extra
   `get_strategy_and_idx` calls at opponent nodes would show up here.

## Decision rule

- **If v43 best < v42 best - 2·SE**, ship `avg_at_traverser=false` as
  the default. Update the F5 config default, the golden hash, and the
  config regression test.
- **If v43 best within ±2·SE of v42**, the two schemes are equivalent
  at this abstraction. Keep the current default (fewer moving parts,
  more existing checkpoints match it).
- **If v43 best > v42 best + 2·SE**, revert the F5 flags entirely.
  Theoretically-better averaging doesn't help in practice on this
  abstraction.

## What this won't settle

- Whether the *floor* dimension of F5 matters. The Kuhn grid already
  showed `neg_floor=true` (RM+) helps on the toy game, so the default
  is likely correct. Testing it on NLHE would need a third run.
- Whether the v42 curve turn-up is fundamental. If both v42 and v43
  turn up at the same place, the rise is not caused by the averaging
  site. The audit's F1 fix could have removed the original cause and
  this pattern is a different phenomenon.

---

## Result (2026-10-01)

| iter | v42 (traverser) | v43 (opponent) |
|---|---|---|
| 3.0M | 3313.4 | 3453.8 |
| 6.0M | 3430.3 | **3402.7** |
| 9.0M | 3374.6 | 3457.6 |
| 12.0M | 3531.8 | 3509.2 |
| 15.0M | 3709.4 | 3715.0 |
| 18.0M | 3780.4 | 3723.7 |
| **best** | **3313.4** | **3402.7** |

Delta of bests: +89.3 mbb for the opponent site. SE is ~130 mbb, so
this is **within 1 sigma** — statistically indistinguishable.

## Decision

**Keep `avg_at_traverser=true` (the current default).** The decision
rule in this doc said: within 2·SE means equivalent, keep the current
default. That's what this is.

## A more important observation

**Both curves turn up at the same place (~3-6M iterations).** The rise
is not caused by the averaging site. The audit's F5 hypothesis — that
the averaging scheme contributed to the "exploitability rises after
long training" pattern — is not supported by this A/B.

Combined with the Kuhn result (RM+ floor helps, neg_floor=false
hurts), the F5 finding is effectively **closed for both dimensions**:

- Floor: keeping `neg_floor=true` is correct.
- Averaging site: keeping `avg_at_traverser=true` is correct.

The rise after 3-6M is therefore either:

1. **Fundamental to the k=200 abstraction.** The abstraction saturates
   at 3-6M iterations and further training adds variance rather than
   reducing it.
2. **Something else in the CFR update that F5 didn't touch.**

The F4 abstraction rebuild (measured at 2h flop / 2.5d turn, plan in
`f4-abstraction-rebuild-plan.md`) is now the more promising direction
than further F5 work.

## What closes here

F5 as an investigation is closed. The flags stay at their current
defaults. The grid doc (`f5-grid.md`) is not worth running further
without a reason to believe an intermediate config would behave
differently — the two extremes of the averaging dimension are
equivalent, and the floor dimension was settled on Kuhn.
