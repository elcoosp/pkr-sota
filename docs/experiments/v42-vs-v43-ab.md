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
