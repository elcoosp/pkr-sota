# pkr-sota: Current Status

**Last updated:** 2026-09-22

## What works

- End-to-end pipeline: precompute -> train -> checkpoint -> export -> load -> query.
- `./smoke.sh` proves the whole chain in ~2-5 minutes cold.
- 94 tests pass across 11 binaries, 1 ignored (`load_external_blueprint`).
- DCFR discounting, PCFR+ momentum, GPU shader parity (all in `pkr-cfr`).
- Turn and river abstraction tables ship in the pipeline (`run.sh`).
- Checkpoint save/load and resume are functional.
- **Parallel training scales positively.** See below.

## Parallel training: fixed

Earlier in the project parallel training was slower than single-threaded.
That is no longer true. Measured on Mac Mini M1 with `PKR_PHASE_PROFILE=1`
and `ITERS_PER_SYNC=256`:

```
 threads    it/s       speedup vs t1
 -------    -------    -------------
    1       15635      1.00x
    2       26879      1.72x
    4       33734      2.16x
    8       38657      2.47x
```

At threads=8 that is **3.34 billion iterations/day**, versus roughly
10 million needed for a level-A arena-playable bot. 300x headroom.

### What made the difference

Earlier attempts had tried and failed at: thread-local idx cache,
interleaved atomic arrays, split strategy_sum, parallel flush, and
strategy-batch deferred writes — each was neutral or worse. The actual
fix was architectural:

1. **Batch logical iterations per rayon dispatch.** The old
   `run_iteration_parallel()` forked at the granularity of one CFR
   iteration and paid a serial merge+flush every iteration. Serial cost
   grew *with* thread count (more threads = more items per iteration to
   merge). Replaced with `run_iterations_parallel(n)`, which runs `n`
   iterations per dispatch, amortizing merge+flush by `n`. `ITERS_PER_SYNC`
   in `pkr-trainer` is 256.

2. **Parallel flush.** After dedup every `(idx, action)` key is unique,
   so the PCFR+ read-modify-write per key is race-free. `flush_cpu_batch`
   and `apply_strategy_batch` now dedup serially then `par_chunks` the
   apply.

3. **i64 `strategy_sum`.** i32 with fixed-point ×1000 saturates after
   ~2.1M weighted visits per slot. A hot preflop infoset can hit that
   in a few thousand iterations, at which point the running average
   would silently wrap to negative. Fixed.

4. **Move `is_near_capacity()` out of the per-batch path.** It pins
   papaya; once per 1000 iterations is enough.

The earlier micro-optimizations (idx cache, split strategy_sum) are
still in the code and still help; they just weren't the top of the
Amdahl curve.

### Remaining headroom

Flush is still ~30% of wall time at threads=8. Two levers if that ever
matters:

- Reusable scratch `HashMap` on the table for dedup, so we don't
  reallocate a ~15-30k entry map per batch.
- Deeper batch (ITERS_PER_SYNC 512/1024) — DCFR's discount schedule
  tolerates it fine.

Neither is worth doing unless training time becomes the constraint.

## Known remaining issues (not blocking)

- `crates/pkr-cfr/src/riversolve.rs` is not real CFR (regrets reset
  each iteration). Not called by the trainer.
- `crates/pkr-cfr/src/valuenet.rs` is not wired into anything; its
  `generate_training_data` returns synthetic labels.
- `crates/pkr-fuzz/` and `crates/pkr-exploit/` are defined in the
  workspace but not integrated into the training or serving path.

## How to run a real training job

```bash
# Quick end-to-end check
./smoke.sh

# Real training
./run.sh
```

Watch the progress line: `iter N/M | infosets: X | Y it/s | ETA Zh`.
Ctrl-C is safe; rerunning `./run.sh` resumes from `train.ckpt`.

Override threads with `--threads N` on the trainer if you want to
benchmark a specific config.
