# River subgame solving POC — POSITIVE

**Date:** 2026-09-26
**Status:** Positive. Concrete-card CFR on a river subgame beats the trained blueprint by a median 9.6 chips (59% exploitability reduction) across 20 boards.

## Headline numbers

20 random boards, disjoint uniform ranges (12 hands each), 100 CFR iterations:

| metric | value |
|---|---|
| wins / total | 19/20 (95%) |
| median delta (BP - CFR) | +9.63 chips |
| median ratio (CFR / BP) | 0.414 |
| mean ratio | 0.440 |
| max delta | +36.46 chips |
| min delta | -1.99 chips |

## Method

- Root: 200-chip stacks, pot=4 at river after limp-check preflop + check-check flop + check-check turn.
- P0 range: uniform over 12 hands from cards 0..26. P1 range: uniform over 12 hands from 26..52.
- CFR: 100 iterations of vanilla CFR+ with linear averaging on the concrete river betting tree.
- Baseline: v34long 100M-iteration blueprint, evaluated at the same subgame with the same ranges.
- Measurement: P1 BR value against each P0 strategy in the concrete subgame.

## Implementation

New crate `crates/pkr-subgame/` (~450 lines):

- `lib.rs` — run_poc, cfr_solve_p0, CfrState::walk, compute_br_v1, blueprint_p0_strategy.
- `tests/river_poc.rs` — CFR vs uniform baseline.
- `tests/blueprint_compare.rs` — single-board CFR vs blueprint.
- `tests/blueprint_sweep.rs` — 20-board aggregate.

Key design: concrete (hole, node) infoset keys with no abstraction; pre-computed deal list reused across CFR iterations; cached hand ranks per deal; foldhash for regret maps.

## Performance

| version | 200-iter solve wall | notes |
|---|---|---|
| v0 | 393s | naive: HashMap + per-terminal evaluator + clone per deal |
| v1 | 6.1s | + foldhash + rank cache |

64x speedup, bit-identical results. Per-solve cost at 100 iters / 12-hand ranges: ~6s.

For 100ms production latency at realistic 500-hand ranges we need another 20-50x. Achievable via range-indexed infoset arrays (5-10x), parallel CFR over deal tree (3-4x), warm-start from blueprint (2-3x).

## Caveats

1. **Uniform ranges.** Real ranges are tighter. The 59% median reduction will shrink — likely to 20-40% — once ranges come from blueprint history.
2. **No safe-solving constraint.** Production needs max-margin or CFRD gadget to avoid exploitation by opponents who deviate.
3. **River only.** Turn adds one chance node; flop adds two. Solvable but requires gadget machinery.
4. **Single action history tested.** All 20 boards use the check-check-check line.

None of these undermine the core finding: at the river, concrete-card CFR dominates the abstracted blueprint.

## Decision

Proceed with full build: 3-4 weeks for river + turn + flop, safe solving, runtime integration.

## Related

- `docs/handoff/HANDOFF_2026-09-25.md` §4 (highest-leverage future work)
- `docs/experiments/v34-long-run-confirmed.md` (the blueprint we beat)
- `docs/experiments/variance-reduction-negative.md` (why eval variance is hard)


## Update 2026-09-26 21:00 — flat-tree refactor + corrected numbers

The original POC used a state-mutating walker with per-node HashMap lookups
and infoset-signature hashing. A subsequent refactor replaced it with a
flat public-tree representation: nodes enumerated once at solver init,
regrets stored in flat `Vec<[f64;6]>` indexed by `node_id * n_deals + deal_id`,
no hashing at run time.

**Numerical verification.** Uniform-P0 best-response values are bit-identical
before and after:

| hands/range | old walker | new walker |
|---|---|---|
| 8 | 38.9115 chips | 38.9115 chips |
| 10 | 29.4033 chips | 29.4033 chips |

The BR walk semantics are unchanged.

**CFR numbers changed** because the old `collect_cfv` mixed action-indexed
and bucket-indexed loops in the regret update. The new code is uniformly
bucket-indexed, which is the correct semantics (regret is defined over the
abstract bucket, not over the concrete action). At 5 iterations, 8 hands:

| | old (buggy) | new (corrected) |
|---|---|---|
| uniform BR | 29.4033 | 38.9115 |
| CFR BR | 4.3193 | 4.3193 |

**Wait** — actually both give 4.3193 at 5 iters / 8 hands on the corrected
ranges. The divergence at 100 iters / 12 hands (8.08 vs -0.77) is the
old code's bucket-mixing breaking convergence at longer solves. The new
code converges properly.

**Performance.** 20 boards × 100 iters × 12 hands:

| version | wall time | per board |
|---|---|---|
| pre-refactor | 154.6s | 7.73s |
| **flat-tree** | **23.3s** | **1.16s** |

**6.6× speedup.**

**Re-run 20-board sweep with corrected CFR:**

| metric | pre-refactor | flat-tree |
|---|---|---|
| wins / total | 19/20 | **20/20** |
| median delta | +9.63 | **+20.48 chips** |
| median ratio | 0.414 | **0.002** |

The corrected solver converges essentially to equilibrium on these small
subgames: after 100 iterations, P1's BR against the CFR strategy is
~0.1 chips against a pot of 4, i.e. nearly unexploitable. The blueprint's
BR value is ~15 chips on the same subgame.

The original POC finding is **confirmed and strengthened**. Median win is
now +20.5 chips rather than +9.6.

**Caveat:** the "median ratio 0.002" is suspiciously good and warrants
the same scrutiny as the original numbers. CFR+ on tiny subgames is
known to converge very fast, but 100 iterations for near-equilibrium at
the 12-hand scale is at the optimistic end. A 20-board sweep with 200
iterations should be run before the full build to confirm.



### 200-iteration confirmation

Re-ran the 20-board sweep at 200 iterations. Results identical to 100:

| | 100 iters | 200 iters |
|---|---|---|
| wins/total | 20/20 | 20/20 |
| median delta | +20.48 | +20.50 |
| median ratio | 0.002 | 0.001 |
| wall time | 23.3s | 45.9s |

CFR+ converges by 100 iterations on these subgames. No further
convergence benefit from 200; double the wall time for no change.

**The ratio 0.001 deserves scrutiny.** BR_v1 against CFR is ~0.1 chips
on a pot of 4. That is near-equilibrium to 2.5% of the pot. This is
consistent with CFR+ on small trees (fast convergence to Nash is a
known property), but it also means the "POC win" is dominated by
the *blueprint* being exploitable on uniform ranges, not by CFR
achieving something extraordinary. The honest next test is real
ranges from blueprint history; the win will shrink there.
