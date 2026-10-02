# pkr-sota: Architecture Overview

**Last updated:** 2026-09-22

This document describes the architecture as implemented. For current
implementation status see `status.md`. For aspirational design see
`spec/architecture.md`.

---

## 1. Constraint model

| Dimension | Training host (Mac Mini M1 16 GB) | Runtime host (cheap VPS or embedded) |
|---|---|---|
| CPU | 4× Firestorm P-cores + 4× Icestorm E-cores | 2 shared vCPU or host process |
| Memory | 16 GB unified | 2–4 GB DDR4 or shared with host app |
| Role | Precompute abstractions + train blueprint | Frozen lookups |
| Constraints | Must not exceed 12 GB resident | Must not exceed 50 MB resident |
| Cost | Sunk | €0 – €5/month |

Two consequences drive everything:

1. **No training on the runtime host.** All CFR logic, precompute
   binaries, and mmap'd abstraction tables live on the training side.
   `pkr-runtime` links nothing from `pkr-cfr`, `pkr-abstraction`, or
   `pkr-eval`.
2. **The runtime artifact is self-describing and O(log n).** A binary
   search over ~10⁶ sorted u64 keys with mmap'd CDF bytes, no allocations,
   no floating point.

---

## 2. Pipeline

```
┌─────────────────────────────────────────────────────────────────┐
│ TRAINING (M1)                                                    │
│                                                                  │
│  pkr-eval ─── hand_ranks.bin    (2.6 M u32 = 10 MB)              │
│  pkr-abstraction                                                 │
│    ├── centroids.bin            (k centroids, 2×f32)             │
│    ├── preflop_abstraction.bin  (1,326 u8)                       │
│    ├── flop_abstraction.bin     (25,989,600 u8 = 26 MB)          │
│    ├── turn_abstraction.bin     (305,377,800 u8 = 305 MB) [opt]  │
│    └── river_buckets.bin        (2,598,960 u8 = 2.6 MB)          │
│                                                                  │
│  pkr-cfr::Trainer                                                │
│    ├── CompactRegretTable                                        │
│    │     ├── data: Vec<AtomicI64>      (regret + momentum interleaved) │
│    │     ├── strategy_sum: Vec<AtomicI64>                        │
│    │     ├── hash_to_idx: PapayaMap<u64, usize>                  │
│    │     └── thread-local idx cache                              │
│    ├── traverse (batched, chunked via rayon)                     │
│    ├── flush_cpu_batch (sort + parallel apply)                   │
│    ├── apply_strategy_batch (sort + parallel apply)              │
│    └── metrics (nodes, depth, cache, dedup, timings)             │
│                                                                  │
│  pkr-export::write_blueprint                                     │
│    └── blueprint.bin (keys + CDFs, mmap'd by runtime)            │
└─────────────────────────────────────────────────────────────────┘
                              │
                    artifact (blueprint.bin)
                              │
┌─────────────────────────────────────────────────────────────────┐
│ RUNTIME (VPS or host app)                                        │
│                                                                  │
│  pkr-runtime                                                     │
│    ├── MmapReader::new(path)     → mmap, parse header            │
│    ├── SolverHandle::new(reader) → cheap wrapper                 │
│    └── get_advice_fast(hash)     → O(log n) lookup + CDF read    │
└─────────────────────────────────────────────────────────────────┘
```

The artifact is the only interface between the two halves.

---

## 3. Training side

### 3.1 CFR algorithm

`pkr-cfr` implements external-sampling CFR with:

- **Regret matching**: at each infoset, current strategy is the positive
  part of accumulated regrets, renormalized. Uniform when all regrets are
  non-positive.
- **Discounted CFR**: canonical (Brown & Sandholm 2019). Regrets are
  multiplied by `t^p / (t^p + 1)` for `t ≥ τ=1000`. `α=1.5`, `β=0.0`.
- **PCFR+ momentum** (Farina, Kroer, Sandholm 2021): the increment is a
  smoothing of the raw regret delta over iterations.
- **Average strategy**: unweighted own-reach sum, accumulated in i64
  fixed-point at scale 1000.

The discount factor is bounded in [0.5, 1) so it cannot overflow. In f32,
it saturates to 1.0 once `t^p > 8.4e6`, meaning the discount is only
active in a narrow window around t = 10³–10⁴. See `status.md` for the full
finding.

### 3.2 Compact regret table

```
data:   [ r0 m0 r1 m1 r2 m2 r3 m3 r4 m4 r5 m5 ] × capacity   (i32)
         └─ regret and momentum interleaved for locality
strategy_sum: [ s0 s1 s2 s3 s4 s5 ] × capacity              (i64)
hash_to_idx: PapayaMap<u64, usize>
```

- **Regret and momentum are interleaved.** Written by the coordinator
  only (flush_cpu_batch), so no false sharing between neighboring
  infosets.
- **Strategy sums are separate.** Written atomically from every thread, so
  keeping them in a separate array eliminates false sharing.
- **Fixed-point i32 for regrets** at scale 1000. Range ±2.1M, which is
  ~100x more than typical regrets reach. Regrets are clipped to `[0, 2.1M]`
  after each update, so no overflow.
- **i64 for strategy sums.** An i32 would saturate after ~2.1M weighted
  visits to a hot infoset.

### 3.3 Traversal

`traverse` is recursive, one pair of calls per iteration (hero perspective
and villain perspective). At each node:

1. If the street is complete and non-terminal, deal community cards from
   a stack-allocated deck.
2. Compute the infoset hash from `(hole, board, history_signature, street)`.
   The `history_signature` is a compact 24-bit value: `(actions_this_street,
   num_raises_this_street, last_was_bet)`. This bounds the infoset count.
3. Get current strategy (via thread-local cache on hit).
4. If acting player is the traverser: recurse into every legal action,
   accumulate regret deltas into a thread-local `Vec<BatchItem>`, push
   strategy ops into a thread-local `Vec<StrategyOp>`.
5. Otherwise: sample one action and recurse once.

Regret updates are **not applied during traversal**. They are accumulated
and applied in `flush_cpu_batch` after the batch completes.

### 3.4 Batched parallel training

`Trainer::run_iterations_parallel(n)`:

1. Reserve `n` iteration numbers with one atomic op.
2. Split into chunks of 16 iterations.
3. Rayon work-steals chunks across threads; each runs into its own buffers.
4. Merge all buffers on the coordinator.
5. `apply_strategy_batch`: sort by `(idx, action)`, walk groups, add to
   `strategy_sum`.
6. `flush_cpu_batch`: sort by `(idx, action)`, walk groups, apply PCFR+
   regret update per unique key in parallel.
7. Fold per-thread `LocalMetrics` into global.

`ITERS_PER_SYNC = 256`. Larger batches amortize the serial merge further
at the cost of staler discount-schedule timing (DCFR tolerates this).

### 3.5 Abstraction

`KMeansAbstraction` maps `(hole, board, history, street)` → u64 hash:

- **Preflop**: `(hole_card_1, hole_card_2)` → 1,326 flat index → centroid id.
- **Flop**: `(combinadic_rank_5, hole_mask_index)` → 25,989,600 flat index
  → centroid id.
- **Turn**: `(combinadic_rank_6, hole_mask_index)` → 305,377,800 flat index
  → centroid id.
- **River**: exact hand rank `>> 6` combined with a board bucket → bounded
  bucket count.

When a lookup table is not loaded for a street, the abstraction falls back
to computing EHS via Monte Carlo at runtime. This is ~100x slower and logs
a warning the first time it happens. All production paths load the tables.

---

## 4. Runtime side

`pkr-runtime` is a library, not a server. The host application loads it
and queries it directly.

### 4.1 Artifact format

`blueprint.bin` (little-endian):

```
offset  size                field
0       32                  FileHeader
                                 magic = "PKRSOTA1"
                                 version = 2
                                 variant_id = 0
                                 infoset_count: u64
                                 max_actions_k: u8
                                 hash_algo: u8 (2 = FNV-1a-64)
32      4                   key_count: u32
36      4                   cdf_size: u32
40      key_count * 8       keys: u64 (ascending)
...     cdf_size            cdf: u8  (K bytes per key, monotonic)
```

### 4.2 Lookup

`SolverHandle::get_advice_fast(hash)`:

1. Binary search the sorted key array.
2. If found, read K bytes from the CDF array.
3. Return `SotaAdvice { cdf_probabilities: [u8; 16], len: K }`.

No allocations, no atomic ops, no locks. Bounds-checked.

Cost: ~21 comparisons for 1M keys, well under 1 µs on the VPS.

### 4.3 Rejected alternatives

The FMph structure built by `pkr-export::fmph` would give O(1) lookup
instead of O(log n), but the current binary search is already fast enough
for the ASR target (< 1 ms). FMph is kept in the writer for a future
O(1) path; the runtime does not use it.

---

## 5. Crate boundaries

```
pkr-contracts (no deps)       ─── trait interface
     ▲
     │
pkr-core ─── Card, Deck, GameState
     │
pkr-eval ─── hand evaluation
     │
pkr-abstraction ─── EHS + k-means + flat indices
     │
pkr-cfr ─── Trainer, CompactRegretTable, traverse, metrics
     │
pkr-export ─── writer, header, fmph, translate
     │
pkr-runtime ─── MmapReader, SolverHandle
```

`pkr-trainer` binaries wire the training chain. `pkr-testgames` is a
standalone harness that depends only on `pkr-cfr`.

`pkr-exploit` and `pkr-fuzz` are defined crates that are not currently
part of either path. They compile and have tests but nothing calls them.

---

## 6. Test strategy

| Layer | What | Where |
|---|---|---|
| Unit | Card, deck, evaluator, dcfr, translate | Various crate `#[cfg(test)]` |
| Integration | Pipeline end-to-end | `binaries/pkr-trainer/tests/pipeline.rs` |
| Runtime contract | Blueprint roundtrip | `crates/pkr-runtime/tests/roundtrip.rs` |
| Smoke | Whole pipeline, real files | `smoke.sh` |
| Profile | Production-scale, metrics | `proftest.sh` |
| Convergence | CFR algorithm on Kuhn | `crates/pkr-testgames` |

Currently no coverage for: the exploit overlay (`pkr-exploit`), the
fuzzer (`pkr-fuzz`), or the GPU path (`pkr-cfr::gpu`).

---

## 7. Known open questions

1. **Does the trained blueprint actually play poker?** The pipeline is
   verified but the bot has not been played. This is the next check.
2. **Does the abstraction resolve enough?** `stats.json` reports the
   strategy distribution (pure/mixed/empty). If most infosets are pure,
   the abstraction is too coarse and quality is capped by k.
3. **Is the batching hurting convergence?** 256-iteration batches collapse
   regret deltas to one update per infoset. Theoretically safe; not
   empirically verified on NLHE.
4. **Is canonical DCFR actually adding anything?** Given f32 rounding, it
   may be pure vanilla CFR for most of the run. Not a bug, but not DCFR
   either.
