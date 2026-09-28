# Training nondeterminism (2026-09-28)

**Status:** NEW BUG — invalidates same-seed A/B comparisons

## Discovery

Two runs launched with **identical configuration, identical seed (202),
identical tables, identical eval cadence** diverged by iteration 5M:

| metric @ 5,007,360 | 30M run | 100M run |
|---|---|---|
| expl_mbb | 2776.55 | 2743.17 |
| infosets | 1,005,012 | 1,002,518 |
| br0 | 6.2377 | 6.3105 |
| br1 | 4.8685 | 4.6622 |

The **infoset count differs by 2,494 (0.25%)**. If training were
deterministic, both runs would visit exactly the same states and the
counts would match.

## Impact

- **Every same-seed A/B in the project is invalid** for differences
  smaller than the run-to-run divergence. The divergence at 5M is
  ~33 mbb in the reading and 0.25% in the state space; over a full
  run it could be much larger.
- The v38 30M-vs-100M finding (+350-375 mbb) is *probably* still real
  — the magnitude is 10x the seed-level divergence we see here — but
  it can't be cleanly attributed without fixing this.
- The v33 preflop feature win (+425 mbb) is also 10x larger than the
  divergence, so likely robust. But smaller wins (like the v36
  capacity win at -56 mbb) are questionable.

## Root cause (suspected)

Rayon parallelizes over deals within an iteration. The final regret
merge into the shared table is order-dependent for **float
accumulation**: `(a + b) + c ≠ a + (b + c)` in f32/f64.

If the merge order depends on thread completion order, the accumulated
regrets differ run-to-run even with identical inputs.

## Verification needed

1. **Run with `--threads 1` twice.** If the results are bit-identical,
   nondeterminism is confirmed as thread-related.
2. **Run with the same thread count twice.** If they still differ, the
   bug is elsewhere (RNG seeding, HashMap iteration order, timers in
   the strategy path).

## Fix candidates

- **Deterministic merge order.** Sort batch items before accumulating.
  Cost: some CPU. Fully fixes the problem.
- **Fixed-precision accumulation.** Round each thread's partial sum
  before merging. Reduces (doesn't eliminate) the divergence.
- **Document and accept.** Report A/B deltas with an added
  "nondeterminism floor" of ±X mbb. Requires measuring X.

## Action

**Do not launch more A/Bs until this is characterized.** The 33 mbb
divergence at 5M is the current lower bound; the true floor across
full runs is unknown. Once measured, every existing result can be
re-evaluated with the correct error bar.


## Verified: nondeterminism is thread-order (2026-09-28)

Two identical runs, single-thread, 2M iterations, seed 202:

| iter | run_A | run_B |
|---|---|---|
| 1,003,520 | 3457.9916 (SE 146.0125, br0 8.7118, br1 5.1202) | 3457.9916 (identical) |

**Bit-identical to the last digit on every field.** Single-thread
training is fully deterministic.

Therefore the divergence we saw at 8 threads (0.25% different infoset
counts at iter 5M) is **rayon thread-order** affecting float
accumulation in the regret merge. `(a+b)+c != a+(b+c)` in f32/f64, and
the merge order depends on which worker thread's batch arrives first.

## Fix

**Deterministic merge.** Sort the per-deal or per-batch contributions
by a fixed key (deal index, or batch index) before accumulating into
the shared regret table. Cost: a sort of the batch vector per sync,
which is O(N log N) on a small N. Estimated impact: <5% throughput.

Alternative if throughput matters: accumulate per-thread partial sums
into a deterministic reduction tree (rayon's `.reduce()` with a
specified associativity gives this for free if the merge op is
replaced with a two-phase accumulate).

## Impact on historical A/Bs

The 8-thread divergence is at least **0.25% in state space**. In eval
readings that showed as ~33 mbb at 5M iterations. Across a full 30M
run, the true floor is unknown but likely in the 30-100 mbb range.

**Interpretation:**
- Deltas < 100 mbb from any 8-thread A/B are unreliable
- Deltas > 300 mbb (like the 30M/100M finding at ~350 mbb) survive
  the noise floor
- The v33 preflop feature win (+425 mbb) also survives

**Recommendation:** until deterministic merge is implemented, run
all A/Bs at `--threads 1`. 8 threads for exploratory sweeps where
direction is enough; single thread when the magnitude matters.

## Single-thread throughput cost

run_A/run_B timing: 2M iters in ~2400s = ~833 it/s.
8-thread throughput: ~7000 it/s.

Single-thread is ~8x slower. Not viable for production training, but
fine for characterization runs.


## FIXED: deterministic batch sort (2026-09-28)

Root cause identified and fixed. Both `par_sort_unstable_by_key` calls
in `crates/pkr-cfr/src/table.rs` used non-total sort keys:

  flush_cpu_batch_with:  (index, action, iteration)
  apply_strategy_batch:  (index, action)

`par_sort_unstable_by_key` reorders items with equal keys arbitrarily.
Since the sequential fold sums f64 in sorted order, different orderings
produced different low bits of the accumulated regret.

**Fix**: append the value's bit pattern as a tiebreaker:
  flush_cpu_batch_with:  (index, action, iteration, delta.to_bits())
  apply_strategy_batch:  (index, action, prob.to_bits())

This makes the sort key a total order. Cost: +4 bytes per key. No
runtime overhead beyond the comparison.

### Verification

Two identical 8-thread runs, 3M iters, seed 202:

  run_C and run_D exploitability.csv — **byte-identical**
  run_C and run_D metrics.csv        — **byte-identical**

The 8-thread nondeterminism is fully resolved. Any A/B at any thread
count now measures only the variable under test.
