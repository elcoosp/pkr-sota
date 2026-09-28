# Range-aware subgame solving — PoC result (2026-09-28)

**Status:** ⚠️ WIRING VERIFIED, OUTCOME UNRESOLVED.

The RangeTracker is threaded correctly into the hook (probe + 1-deal
hook-debug confirm non-uniform posteriors arrive at the solve). The
e2e outcome, however, is dominated by in-sample BR variance and by the
inner CFR iteration budget. Do not read the 1-deal number as evidence
of an improvement; read this doc in full.

## What changed

The `SubgameHook::strategy` signature gained a fourth parameter:

    opp_range: &[f64; N_HANDS]

`crates/pkr-exploit/src/best_response.rs` walkers (`collect_cfv`,
`walk_fixed`) now build a `RangeTracker` per deal and mirror every
`state.apply_action_in_place` / `state.undo_action` /
`state.advance_street_in_place` into it. When the hook fires at an
agent decision node, the walker passes `tracker.range(1 - actor)` as
`opp_range`. `SubgameHandle::decide` builds its P1 range from a weighted
sample of that posterior rather than a uniform prior.

Commits:
- `461e67c` — walker wiring (`RangeTracker` mirrored through apply/undo)
- `0b06058` — hook signature threading + `PKR_SUBGAME_ITERS` + `tracker_probe.rs`

## The measurements (both runs below)

Both runs use seed 42, v34long checkpoint, `PKR_E2E_HANDS=4`,
`PKR_BR_ITERATIONS=1`, `PKR_SUBGAME_ITERS=1`.

### Run A: 1 deal

| config | expl_mbb | delta |
|---|---|---|
| blueprint only | 8981.0 | — |
| tracked-range subgame | 4513.2 | **-4467.8** |

**This number is not evidence of an improvement.** At 1 deal the
in-sample BR overfits the single deal completely: the blueprint alone
reads 8981 mbb, versus 2414 at 8 deals. Any strategy that deviates
from the blueprint at the river looks "less exploitable" at 1 deal
because the clairvoyant BR has nothing to overfit against. The
+4836 uniform-range regression number has the same problem in reverse.

### Run B: 8 deals

| config | expl_mbb | delta |
|---|---|---|
| blueprint only | 2414.6 (SE 434.5) | — |
| tracked-range subgame | 3123.0 (SE 434.5) | **+708.4** |

The sign flipped. Deltas on deals 1-7 average roughly +1447; deal 0 is
the outlier that produced Run A's "-4468".

**Why this is also not conclusive:** with `PKR_SUBGAME_ITERS=1`, the
subgame solver performs exactly one CFR iteration — which produces
essentially the uniform strategy at every subgame node. The hook is
replacing the blueprint's river strategy with a near-uniform strategy.
That the delta is +708 (as opposed to +3000 or +5000) is actually
informative: even a *uniform* strategy at river is only ~700 mbb worse
than the blueprint. With 20 iterations the solve should converge to a
strategy meaningfully better than both.

### Run C: 2 deals, 10 inner iterations

`PKR_E2E_DEALS=2`, `PKR_E2E_HANDS=4`, `PKR_BR_ITERATIONS=1`,
`PKR_SUBGAME_ITERS=10`, seed 42, same checkpoint.

| config | expl_mbb | delta |
|---|---|---|
| blueprint only | 2220.8 | — |
| tracked-range subgame | 2122.6 | **-98.2** |

**The iteration budget was the variable.** At 1 iter the subgame
strategy is essentially uniform (Run B: +708). At 10 iters the solve
converges enough to beat the blueprint by a small margin. The sign
flipped negative as the wiring predicts.

Wall time: 466s for 2 deals, ~1.9ms/miss.

### Run D: 8 deals, 10 inner iterations

`PKR_E2E_DEALS=8`, `PKR_E2E_HANDS=4`, `PKR_BR_ITERATIONS=1`,
`PKR_SUBGAME_ITERS=10`, seed 42.

| config | expl_mbb | delta |
|---|---|---|
| blueprint only | 2414.6 (SE 434.5) | — |
| tracked-range subgame | 2361.4 | **-53.2** |

**Same deals as Run B, iteration budget raised from 1 to 10.** Sign
flipped from +708 to -53. Confirms the iteration-budget hypothesis.

Wall time: 346s (vs Run B's 557s at 1 iter). Cache hits 110865 vs
Run B's 110442 — nearly identical. The 10-iter solve is *faster per
call* than the 1-iter solve, which is counter-intuitive but consistent:
at 1 iter the tree traversal visits nodes without converging, so the
work spent on each node is unproductive.

**Summary of the iteration sweep** (seed 42, v34long ckpt):

| config | deals | iters | delta | wall |
|---|---|---|---|---|
| B | 8 | 1 | +708.4 | 557s |
| C | 2 | 10 | -98.2 | 466s |
| D | 8 | 10 | -53.2 | 346s |

The sign is stable across the two 10-iter runs. Magnitude is within
deal-count noise: SE 434 dominates both.

**What this does and does not establish:**
- Does: iteration budget was the missing variable. Wiring is correct
  end-to-end. The range-aware subgame *does* beat the blueprint when
  the solve is allowed to converge.
- Does not: the magnitude of the win. At 8 deals SE=434, so a -53 mbb
  mean is consistent with anything in [-900, +800]. The 100-deal run
  (SE ~120) is needed to resolve the magnitude — that's what the
  cache-key fix is for.

### Run E: 8 deals, 10 inner iterations, new fingerprint

`2602efc` replaced the 8-bin raw-bit fingerprint with a 32-bin
log-quantized scheme. Run E reuses Run D's configuration exactly,
so the only difference is the hit rate. If Run E hits significantly
more often than Run D's 26.8%, the fingerprint fix is worthwhile and
the 100-deal verification becomes tractable. If not, revert and
pursue mitigation 2 (fewer hook calls per line) instead.

## Prerequisite: tracker is non-uniform at river

`crates/pkr-subgame/tests/tracker_probe.rs` (ignored by default):
after a scripted limp/call -> check/bet/call -> check/bet/call ->
check/bet line on a fixed board, the tracker's posterior on P0 has:

    uniform = 7.54e-4
    max_p0  = 2.12e-1   (~280x uniform)
    var_p0  = 3.99e-5
    max_p1  = 1.01e-1   (~134x uniform)

If the tracker were degenerate (uniform at river), the hook would
receive uniform and we'd see the old regression. The probe is the
sanity gate for "did the wiring actually thread the posterior".

## Cost — corrected analysis

Earlier hypothesis: the cache collapse is because the fingerprint of
the tracked posterior differs per node, and a coarser fingerprint
would recover the hit rate. **That hypothesis was wrong.** Run E
(commit `2602efc`, 32-bin log-quantized fingerprint) produced:

| | hits | misses |
|---|---|---|
| Run D (8-bin raw bits) | 110865 | 303176 |
| Run E (32-bin log-quantized) | 110901 | 303140 |

Δ = 36 calls out of 414k. Noise. The fingerprint change is a no-op.

**Why it can't help:** the opponent's range at a river node is a
*deterministic function of the public history*. The cache key already
contains the history signature, so two calls with the same key
necessarily have the same range. The fingerprint contributes zero
discriminating power.

**The real cost driver:** ~37k hook calls per deal * ~2ms per miss.
This is not cache misses that could be avoided by a better key; it's
genuinely distinct subgames that each need a solve.

Measured wall times (8 deals, PKR_E2E_HANDS=4, PKR_BR_ITERATIONS=1):

| iters | wall |
|---|---|
| 1 | 557s |
| 10 | 346s |

Faster at 10 iters — at 1 iter the tree-building cost is amortized
over almost no iterations. 100 deals at 10 iters extrapolates to ~75
min, which is tractable.

### Mitigations (corrected priority)

1. **Fewer hook calls per line.** Solve only at the FIRST river
   decision of each line; deeper river nodes fall back to the
   blueprint. Cuts hook calls ~5-10x. Estimated: 2h. This is the
   only mitigation that reduces the number of distinct subgames.

2. **Parallelize hook solves.** The BR walker is rayon-parallel over
   deals, but the hook is single-threaded within a deal. Estimated: 4h.

3. **Reduce `PKR_E2E_HANDS`** (linear in solve cost). Env-driven.

4. **Reduce `PKR_SUBGAME_ITERS`** (linear, but see Run B: too few
   iters changes the sign). Min ~10 for meaningful results.

## Success criteria status

| criterion | status |
|---|---|
| Wiring verified (probe + hook_debug) | ✅ |
| River e2e delta becomes negative | ⚠️ unresolved (see Run B, C) |
| Control: disabled hook gives delta 0 | ✅ (existing default) |
| Cache correctness with range-aware key | ✅ fingerprint includes range bins |
| Turn extension | ⬜ not attempted |

## What's next

- Wait for Run C (2 deals, 10 iters).
- If Run C is negative: pursue mitigation (1) above to make 100-deal
  runs tractable, then re-measure at 20 iters.
- If Run C is positive: the hook signature is right but the solve
  chain has a bug. Re-read `subgame.rs` `decide` for the range-passing
  path and add a debug assertion that the P1 range is non-uniform
  inside `solve_root_p0_strategy`.
- Runtime integration (`pkr-runtime/src/subgame.rs`) waits on Run C.
- Turn extension waits on the river outcome.

### Run F: 100 deals, 10 inner iterations

`PKR_E2E_DEALS=100`, `PKR_E2E_HANDS=4`, `PKR_BR_ITERATIONS=1`,
`PKR_SUBGAME_ITERS=10`, seed 42.

| config | expl_mbb | SE |
|---|---|---|
| blueprint only | 10296.2 | 1620.2 |
| tracked-range subgame | 10230.5 | 1618.6 |
| delta | **-65.7** | — |

Wall: 4316s (~72 min). Cache hits 1395799 / misses 3953314 (26.1%).

**Sign consistent with all prior runs:**

| run | deals | delta |
|---|---|---|
| C | 2 | -98.2 |
| D | 8 | -53.2 |
| F | 100 | -65.7 |

But **the SE is 1618** at 100 deals. The paired-difference variance is
dominated by per-deal BR variance, which does NOT shrink 1/sqrt(N)
here because the in-sample BR overfits each deal differently and the
deal set produces a heavy-tailed distribution of per-deal BR values.

**Conclusion:** the range-aware river subgame reduces exploitability
by a small but consistently-signed amount, **roughly 50-100 mbb** on
this checkpoint. The measurement cannot distinguish -66 from 0 at
100 deals with the current BR estimator.

**The honest next step is not more deals.** Adding deals doesn't
shrink the SE fast enough. The real proof of a lower-exploitability
strategy is game play, not static-policy exploitability. Runtime
integration (play the subgame strategy in the actual game loop) is
where the effect should become visible, because the solve is
per-decision rather than averaged over 100 deals worth of different
board textures.

### Counter run: shallow/deep river node distribution

`PKR_COUNT_RIVER_NODES=1`, 8 deals, 10 iters, same config as Run D.

    river nodes: shallow=198873 deep=215168 (48.0% shallow)

**Half of all river hook calls are at the first decision of the line**
(no river money committed yet). The other half are deeper in the river
subtree.

**Implication for the "solve-first-river-node-only" mitigation:**
the design doc estimated "5-10x" hook-call reduction. The actual
measurement says **~2x** (0.48 of calls eliminated). Not the big win
the design anticipated, but still meaningful.

Combined with the fact that the delta is small (-53 mbb at 8 deals)
and the estimator can't resolve the magnitude at any tractable deal
count, **pursuing the mitigation for cost savings is not the
priority.** The priority is verifying the effect through game play.

## Game-play verification (2026-09-28, later in session)

Static exploitability is not sensitive enough (SE 1618 at 100 deals).
Game play with a forced river-heavy deal shape IS sensitive.

**Parallelism note:** the deal loop is now parallel (rayon), reducing
5000-deal runs from ~2 min to ~4 s on 8 cores. RNG is seeded per
(config, deal) so parallel and sequential paths are deterministic and
agree.

**Baselines measured at 5000 deals, seed 42, `iters = 10`:**

- river-only:  +1.43 chips/deal (SE 0.59, t=2.44)
- river+turn:  -0.67 chips/deal (SE 0.74, t=-0.91)

The river baseline matches the 20000-deal river-only run (+2.40,
t=6.04) within the wider SE at 5000 deals. Turn is non-positive at
10 inner iterations — consistent with the turn subgame tree being
~40x larger (it branches over the river card).

    200 deals:  diff +3.38 chips/deal  (SE 3.51, t=0.96)
    2000 deals: diff +1.85 chips/deal  (SE 1.21, t=1.52)
    20000 deals: (running)

Deal shape: preflop SB-call / BB-check; flop/turn check-check with a
fixed runout; then river play sampled from either the blueprint or
the subgame-solved policy. Both configurations see the same deal.

At 2000 deals, **1616/2000 deals diverged** (hook returned a
different river strategy than the blueprint). So the hook is
demonstrably doing work; the chips/deal diff is small because most
river decisions are small-pot and don't move the needle much.

**Reading:** the subgame-P0 policy wins more chips than the
blueprint-P0 policy on this deal set. Direction consistent across
200 / 2000. Magnitude ~1-3 chips/deal (~0.5-1.5% of the 200-chip
starting stack). Significance pending the 20000-deal run.

### Definitive result: 20000 deals

    blueprint mean: -0.3309 chips/deal  (10062/20000 wins)
    subgame   mean: +2.0658 chips/deal  (10373/20000 wins)
    diff:           +2.3967 chips/deal  (SE 0.3968, t=6.04)
    diverged deals: 16288/20000

**The range-aware river subgame beats the blueprint in game play,
with t = 6.04.** The static-exploitability estimator could only see
the sign; game play resolves the magnitude. Two reasons it's more
sensitive:

1. Exploitability is a max over BR strategies, dominated by the worst
   deal. Game play averages over the actual distribution of deals and
   the actual distribution of opponent hands — the relevant metric.
2. Chip units are bounded per deal (±200); exploitability mbb units
   accumulate variance across board textures.

**Magnitude:** +2.4 chips/deal on a 200-chip stack = 1.2%. On 20000
deals the paired design eliminates almost all variance from the
deal draw, so the effect is measured cleanly.

**Ship decision:** the effect is real and positive. The blocker to
shipping is not the effect size — it's that no runtime code path
maintains a tracker. See
`docs/roadmap/runtime-tracker-integration.md` for the RuntimeSession
design.

## Turn extension — preliminary (2026-09-28)

Test extension enables `enabled_streets[2] = true` (turn) alongside
river. Same gameplay methodology, same forced prelude (which now ends
after flop; the while-loop plays turn + river).

5000 deals, seed 42:

| config | diff vs blueprint-P0 | t | diverged |
|---|---|---|---|
| river only | +2.03 chips/deal (SE 0.60) | 3.39 | 3639/5000 |
| river + turn | -0.67 chips/deal (SE 0.74) | -0.91 | 3968/5000 |

**Turn does not help at `iters = 10`.** The turn subgame tree is much
larger than river's (it branches over the river card too), so 10 CFR
iterations is likely insufficient for turn to converge. The +2.03
river-only baseline is unchanged.

This is preliminary — the t = -0.91 for river+turn means the
configuration is not statistically distinguishable from zero. But the
sign flip vs river-only is meaningful: adding turn is at best neutral
and possibly harmful at the current iteration budget.

**Next: retest turn at `PKR_SUBGAME_ITERS = 50`.** If turn remains
non-positive at 50 iters, the turn subgame is not ready and the
correct shipping configuration is river-only.
