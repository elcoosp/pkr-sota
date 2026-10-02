> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

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

  exploitability.csv:  byte-identical
  metrics.csv:         every training-state column identical
                       (infosets, max_abs_regret, mean_abs_regret,
                       strat_mass, nodes, cache_hit_rate, regret_in,
                       regret_out, regret_dedup, strategy_applied)
                       only wall-clock columns differ (wall_s,
                       it_per_s, traverse_ms, merge_ms, flush_ms,
                       wall_ms) — expected, since wall time isn't
                       deterministic.

The 8-thread nondeterminism is fully resolved. Any A/B at any thread
count now measures only the variable under test.

---

## RETEST NOTE (2026-09-29)

The +425 mbb figure cited above is the *pre-determinism-fix* estimate.
The deterministic retest (see the RETEST section in
`docs/experiments/v33-rich-preflop-confirmed.md`) gives:

    seed 42: 2D=3336.0, 6D=3093.1, delta=-242.9  (was -463.8 pre-fix)

The 6D win holds in sign but shrinks ~48%. When quoting a magnitude,
use ~240 mbb, not ~425. The argument in this document about the
nondeterminism floor is unaffected — 240 mbb is still well above the
30-100 mbb noise floor the doc derives.

---

## SECOND SOURCE FOUND (2026-09-29)

The 8-thread fix (00a778d) made `exploitability.csv` byte-identical and
all *reported* training state (infosets, max_abs_regret, mean_abs_regret,
nodes, cache_hit_rate in the CSV) identical. But two 100k-iteration runs
at 4 threads, seed 42, still produce:

    train.ckpt:    DIFFERENT
    metrics.csv:   wall-clock columns differ (expected)
    stats.json:    timing columns differ (expected)
    blueprint.bin: IDENTICAL
    exploitability.csv: IDENTICAL

So `train.ckpt` has a residual divergence that doesn't affect the
shipped artifact.

### Where it is

`stats.json` from the two runs, section by section:

    snapshot.max_abs_regret:     SAME (90536.3828125)
    snapshot.infosets:           SAME (339229)
    snapshot.strategy_sum_mass:  DIFFERS at 1e-11
    strategy_analysis.mean_entropy_bits: DIFFERS at 1e-14
    sample_infosets[*]:          DIFFERS (a non-deterministic sample)
    cumulative_metrics.*:        DIFFERS (timing)
    wall_seconds:                DIFFERS

`strategy_sum_mass` is a sum of every strategy-sum cell across the table.
A relative difference of ~5e-17 per cell accumulates to ~1e-11 over
339K cells. That is float-associativity order-dependence, not a logic
bug.

### Why `blueprint.bin` is still identical

The blueprint exporter quantizes the average strategy to u8 CDFs. A
1e-11 difference in the underlying strategy sum is far below the 1/255
quantization step, so the exported bytes are bit-identical. The host
never sees the divergence.

### Where the divergence is (partial)

`strategy_sum_mass` in the snapshot differs. That metric is
`sum(strategy_sum[cell])` across the table — the accumulation itself.

The strategy batch path *is* already covered by the fix:
`apply_strategy_batch` at `table.rs:520` sorts by
`(index, action, prob.to_bits())`, i.e. the tiebreaker is present.

So the divergence is *not* "the accumulator has no sort fix". The sort
is there. What we know:

1. The regret table's *reported* state (`max_abs_regret`, `infosets`)
   matches bit-for-bit across runs.
2. The exported blueprint (which is a quantized function of
   `strategy_sum`) matches bit-for-bit.
3. But `strategy_sum_mass` itself differs by ~1e-11.

That combination means the underlying `strategy_sum` cells differ at
float precision while the u8-quantized average does not. The source
of the float difference is not yet localized. Candidates that need
ruling out:

- Cross-batch fold order. Each batch is internally sorted, but the
  per-cell accumulator is written once per batch. If two batches
  contain the same cell, the fold is `((cur + delta1) + delta2)` vs
  `((cur + delta2) + delta1)`, which is not associative. The sort
  fixes order *within* a batch; it says nothing about batch order,
  which is determined by iteration index. That should be identical
  across runs at the same iteration count... unless the batch
  boundaries shift.
- Traversal output ordering. The traversal is parallel over deals
  (rayon). Each deal produces its own (idx, action, delta) tuples.
  The *deltas* are deterministic per deal; the *batch* is then
  sorted. If some cross-deal state leaks (e.g. shared RNG, shared
  regret reads that race with writes), the deltas could differ.
- RNG seeding. Each deal's RNG is seeded from the run seed and the
  deal index. Need to verify no `rand::thread_rng` or similar
  sneaks in.

The next step to localize this is a single-threaded run at the same
settings: if single-threaded produces a bit-identical `train.ckpt`,
the divergence is thread-order-dependent and the batch-fold hypothesis
is the leading candidate. If single-threaded also differs, the source
is in the per-deal computation and RNG seeding is the leading
candidate.

**Do not** assume it's the accumulator's sort. That's already fixed.
The claim in this section is a partial localization, not a root cause.

---

## THIRD SOURCE FIXED (2026-09-29): checkpoint map iteration

`save_checkpoint` wrote the (hash, index) map in `PapayaMap`'s
iteration order. PapayaMap's iteration order is implementation-defined
and not stable across runs even with identical insertions. So the
on-disk `train.ckpt` bytes differed every run, even though the
underlying data was the same.

Fix: materialize the map entries, sort by hash, then write. The loader
rebuilds a fresh map from the sequence, so on-disk order was never
semantically meaningful — it just leaked through to the file bytes.

    let mut entries: Vec<(u64, usize)> = guard.iter().map(|(k, v)| (*k, *v)).collect();
    entries.sort_unstable_by_key(|(k, _)| *k);

Committed at `0a3c747`. Verified: two single-threaded 100k-iteration
runs at seed 42 now produce byte-identical `train.ckpt`
(sha256 a26c768196eb7b6d).

## STILL OPEN: 4-thread train.ckpt divergence

At 4 threads, `train.ckpt` still differs between runs of the same
seed. The divergence is in the strategy-sum path:

    SAME  regret_ops_input, regret_ops_unique
    SAME  max_abs_regret, mean_abs_regret, infosets
    DIFF  strategy_sum_mass       (at ~5e-17 per cell)
    DIFF  mean_entropy_bits       (at ~1e-14)

So the regret table is bit-identical across threads; the strategy-sum
accumulator is not. `apply_strategy_batch` already sorts by
`(index, action, prob.to_bits())`, so the within-batch fold order is
deterministic. What remains open:

- The strategy batch is a `&mut Vec<StrategyOp>` shared across
  `par_iter` over deals. The push order across deals is racy.
- If two ops hit the same (index, action) with identical `prob.to_bits()`,
  the sort key ties — but numerically identical values fold
  order-independently, so this shouldn't matter.
- If they have different probs, the sort orders them — so within-batch
  order is fine.

That points to the *batch boundaries* being nondeterministic: if the
per-deal work completes in different orders, ops from deal A and deal
B land in different batches across runs, and `(cur + delta_A) + delta_B`
vs `(cur + delta_B) + delta_A` differ in the low bits.

Confirmed effect: `strategy_sum_mass` differs by ~5e-17 relative,
which accumulates to ~1e-11 over 339K cells. Below the u8 quantization
step in the exporter, so `blueprint.bin` is bit-identical (verified
across 4-thread runs).

Impact: `train.ckpt` is not byte-reproducible at >1 thread. Fixing it
requires either (a) accumulating per-deal partial sums and reducing
them in deterministic order, or (b) serializing the strategy-sum path.
Neither is urgent because the shipped artifact (`blueprint.bin`) and
the reported metrics (`exploitability.csv`) are already deterministic
at 4 threads.

## Summary after 2026-09-29

| thread count | blueprint.bin | exploitability.csv | train.ckpt |
|---|---|---|---|
| 1 | identical | identical | **identical** |
| 4 | identical | identical | differs (strategy-sum) |
| 8 | identical | identical | differs (strategy-sum) |

### What this means for CI

A golden-hash CI test on `train.ckpt` is not achievable at 4 threads in
the current state. Either:

1. **Hash `blueprint.bin`** — it's deterministic and is the shipped
   artifact. Catches any behavioral drift that affects what hosts load.
   Recommended.
2. **Hash `exploitability.csv`** — deterministic. Catches drift in the
   final reading but not in intermediate state.
3. **Find and fix the strategy-sum ordering** — a real but larger task.
   Worth doing eventually for full reproducibility of checkpoints.

We ship the CI test on `blueprint.bin` (option 1). The train.ckpt gap
is documented here and left open.

---

## FOURTH SOURCE (2026-10-02): alloc_idx race orphans slots

`CompactRegretTable::get_or_create_idx` is a check-then-act:

    if let Some(idx) = guard.get(&hash) { return *idx; }
    let fresh = self.alloc_idx();            // advances next_idx
    match guard.try_insert(hash, fresh) {
        Ok(_) => fresh,
        Err(_) => guard.get(&hash).copied().unwrap_or(fresh), // orphan
    }

Two rayon workers that miss the *same* new hash both call `alloc_idx`;
one wins `try_insert`, the other's index is orphaned — allocated, never
mapped, zeroed forever.

**This is NOT the float-accumulation bug** the batch-sort fix
(00a778d) addressed; it is a separate source the earlier analysis did
not attribute.

### Impact

- `snapshot.infosets` reads `allocated()` = `next_idx`. Because racy
  threads both bump `next_idx`, the *reported* infoset count is
  timing-dependent. **This is the "0.25% infoset divergence" the
  original investigation opened with** — the batch-sort fix made
  `exploitability.csv` byte-identical (that was float order), but the
  count race is orthogonal and can still fire.
- `is_near_capacity` (95% of `allocated()`) trips slightly early.
- Wasted capacity: one slot per lost race.
- **No strategy corruption**: orphans are never in `hash_to_idx`,
  never queried, and the checkpoint writes/reads both `allocated()`
  and `len()` consistently.

### Fix (not yet applied — needs a test)

The leak itself is inherent to "allocate index, then insert into a
concurrent map"; no papaya API removes it without a per-key lock. The
*reported metric* can be made deterministic cheaply:

- report `hash_to_idx.len()` (actual map entries) as `snapshot.infosets`
  — deterministic;
- carry `allocated()` in a **separate** field for capacity accounting,
  so `is_near_capacity` stays conservative.

That changes the `TableSnapshot` struct and its consumers (trainer CSV,
stats.json), so it is a follow-up with its own tests — not a hot-path
edit under load.

---

## FOURTH SOURCE (2026-10-02): alloc_idx race orphans slots

`CompactRegretTable::get_or_create_idx` is a check-then-act:

    if let Some(idx) = guard.get(&hash) { return *idx; }
    let fresh = self.alloc_idx();            // advances next_idx
    match guard.try_insert(hash, fresh) {
        Ok(_) => fresh,
        Err(_) => guard.get(&hash).copied().unwrap_or(fresh), // orphan
    }

Two rayon workers that miss the *same* new hash both call `alloc_idx`;
one wins `try_insert`, the other's index is orphaned — allocated, never
mapped, zeroed forever.

**This is NOT the float-accumulation bug** the batch-sort fix
(00a778d) addressed; it is a separate source the earlier analysis did
not attribute.

### Impact

- `snapshot.infosets` reads `allocated()` = `next_idx`. Because racy
  threads both bump `next_idx`, the *reported* infoset count is
  timing-dependent. **This is the "0.25% infoset divergence" the
  original investigation opened with** — the batch-sort fix made
  `exploitability.csv` byte-identical (that was float order), but the
  count race is orthogonal and can still fire.
- `is_near_capacity` (95% of `allocated()`) trips slightly early.
- Wasted capacity: one slot per lost race.
- **No strategy corruption**: orphans are never in `hash_to_idx`,
  never queried, and the checkpoint writes/reads both `allocated()`
  and `len()` consistently.

### Fix (not yet applied — needs a test)

The leak itself is inherent to "allocate index, then insert into a
concurrent map"; no papaya API removes it without a per-key lock. The
*reported metric* can be made deterministic cheaply:

- report `hash_to_idx.len()` (actual map entries) as `snapshot.infosets`
  — deterministic;
- carry `allocated()` in a **separate** field for capacity accounting,
  so `is_near_capacity` stays conservative.

That changes the `TableSnapshot` struct and its consumers (trainer CSV,
stats.json), so it is a follow-up with its own tests — not a hot-path
edit under load.
