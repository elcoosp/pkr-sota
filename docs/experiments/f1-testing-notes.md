> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

# F1 regression testing — three failed approaches and why

**Date:** 2026-09-30
**Context:** after fixing F1 (commit `e43c74f`), the goal was a fast
regression test that would catch a re-introduction of the bug.
Three approaches were tried. None worked. The fourth (a checkpoint
pin) is the one that shipped.

## The bug, in one line

`collect_cfv` accumulated counterfactual values at BR-seat nodes as
`e[a] += cv * deal_prior`, ignoring the opponent's reach. It made the
BR walker evaluate every action against a near-uniform villain range,
which inflated low-sample readings and — critically — made the
estimator's standard error not shrink with more deals.

## Approach 1: pure-SE-shrinkage test on a trivial abstraction

Idea: run `sampled_exploitability` at N=100 and N=400 deals with a
small synthetic abstraction and assert SE(100) / SE(400) ≈ 2
(1/sqrt(N)). If the bug returned, SE wouldn't shrink.

Why it failed:
- The SE ratio was 7.4, not 2. The synthetic abstraction's tree shape
  changes with N, which changes per-deal variance independent of the
  reach-weighting bug. There's no fixed point to test against.
- With a 4-infoset abstraction the test ran in 8 minutes anyway,
  because the estimator walks every deal through every board card.

## Approach 2: two-strategy divergence test

Idea: build two 16-infoset tables by hand, one fold-heavy and one
call-heavy, and assert the estimator's reading differs by more than
the pooled SE.

Why it failed:
- It DID show the difference: fold-heavy 5228 mbb vs call-heavy
  7649 mbb. The estimator sees the strategies.
- At 200 deals the pooled SE was 2668 mbb, larger than the 2421 mbb
  difference. The assertion needs more deals, which means more
  runtime, which means it can't run cheaply.
- More fundamentally, a small synthetic abstraction can't isolate F1.
  With 16 infosets every deal sees the same strategy, so the
  per-deal CFV weighting the fix changed is the SAME weight for every
  deal. The bug's effect requires hand and board diversity, which a
  small abstraction doesn't have.

## Approach 3: large synthetic abstraction

Idea: a realistic board-size abstraction with many distinct infosets.

Why it failed:
- Slow. Even reduced to 4 infosets the test took 8 minutes. A
  realistic abstraction would take hours.
- Same fundamental issue as (2): synthetic deals don't reproduce the
  variance structure that made F1's SE not shrink.

## Approach 4 (shipped): checkpoint pin

`crates/pkr-exploit/tests/f1_estimator_pin.rs`, commit `55c01b9`.

- Loads the post-audit v41 checkpoint.
- Runs `sampled_exploitability` at 1000 deals, seed 42.
- Asserts the reading matches a constant within 1 mbb.
- Marked `#[ignore]` because it needs the ~200 MB checkpoint, which
  is a build artifact.

Why this is the right shape:
- Real data. The v41 checkpoint has the hand and board diversity F1
  actually depends on.
- Fixed parameters. Seed and deal count locked.
- Exact value. Any change to the reach weighting shifts the number.
- One constant to update on intentional change, with a comment
  explaining when that's allowed.

The `RECORDED_MBB` constant is currently NaN (v41 still training).
The test skips itself with a message until the constant is filled in.

## The lesson

F1 is a bug about per-deal accumulation weighting. Its effect lives
in the interaction between real deal diversity and the strategy the
blueprint actually plays. Synthetic tests can't reproduce that
interaction, and realistic tests are too slow to run frequently. The
right regression guard is an ignored integration test on real data
with a pinned reading. Attempts 1-3 are recorded here so that the
next person doesn't spend a session rediscovering them.
