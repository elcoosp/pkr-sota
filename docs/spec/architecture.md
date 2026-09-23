# Architecture & Design Specification — pkr-sota

| Field | Value |
|-------|-------|
| **Project** | pkr-sota |
| **Document** | Architecture & Design Specification (Level 3) |
| **Version** | 2.0 |
| **Date** | 2026-09-22 |
| **Status** | Current |

This spec reflects the codebase as implemented. For the aspirational
original design, see `docs/archive/`.

---

## 1. Context & Scope

### 1.1 Objective

A No-Limit Texas Hold'em solver that trains offline on a Mac Mini M1 and
serves sub-millisecond lookups from a memory-mapped artifact. The runtime
is a library (`pkr-runtime`), not a standalone service.

### 1.2 Scope

**In scope:**
- Discriminated CFR with canonical discount + PCFR+ momentum
- Compact regret table with i32 fixed-point regrets and i64 strategy sums
- Batched parallel training via rayon
- KMeans abstraction with mmap'd flat lookup tables
- Memory-mapped blueprint with sorted keys + CDF bytes
- Instrumentation: per-window metrics, post-training analysis, sampled
  infosets

**Out of scope:**
- WebSocket / HTTP server (host app provides it)
- GUI
- Distributed training
- Real-time subgame solving (riversolve.rs is aspirational)
- Neural value networks (valuenet.rs is aspirational)

---

## 2. Architecturally Significant Requirements (ASRs)

| ID | Requirement | Verified by |
|---|---|---|
| ASR-001 | Runtime lookup p99 < 1 ms | `pkr-runtime::roundtrip` benchmarks (informal) |
| ASR-002 | Runtime memory < 50 MB per variant | `mmap` of a ~10 MB file + a small `SolverHandle` |
| ASR-003 | Training memory < 12 GB on M1 | 50M capacity × 12 fields × 4 B + 50M × 6 × 8 B = ~4.8 GB |
| ASR-004 | ≥ 10⁷ iterations in < 1 hour | measured 27,700 it/s = 40 s for 10⁶ |
| ASR-005 | Runtime exposes a local API, no network | `pkr-runtime` links no network crates |
| ASR-006 | Runtime builds for x86_64 Linux | `cargo check --target x86_64-unknown-linux-gnu` passes |
| ASR-007 | Variant-agnostic core (future PLO etc.) | `GameRules` trait exists but only `NlheRuleset` implements it |

ASR-007 is partially satisfied: the trait boundary exists, but the
abstraction, evaluator, and writer are NLHE-specific.

---

## 3. System overview

### 3.1 Container diagram

```
┌──────────────────────────────────────────────────────────────────┐
│ Training host (M1)                                                │
│                                                                   │
│   pkr-trainer  ────►  pkr-abstraction  ────►  pkr-eval           │
│       │                    │                    │                │
│       ├───►  pkr-cfr  ─────┘                    │                │
│       │        │                                │                │
│       └───►  pkr-export  ───────────────────────┘                │
│                │                                                  │
│                └───►  blueprint.bin                              │
└──────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌──────────────────────────────────────────────────────────────────┐
│ Runtime host                                                      │
│                                                                   │
│   host app  ────►  pkr-runtime  ────►  mmap(blueprint.bin)        │
│                      │                                            │
│                      └───►  SotaAdvice                            │
└──────────────────────────────────────────────────────────────────┘
```

### 3.2 Crate dependencies

`pkr-contracts` is the interface boundary. All other crates depend on it
and nothing above it. `pkr-cfr` does not depend on `pkr-export`.
`pkr-runtime` does not depend on `pkr-cfr`.

---

## 4. Training pipeline

### 4.1 CFR algorithm

External-sampling CFR with DCFR discounting and PCFR+ momentum. See
`arch-overview.md` §3.1 for the full algorithm description.

### 4.2 CompactRegretTable

See `arch-overview.md` §3.2 for the memory layout. Interface:

```rust
impl CompactRegretTable {
    fn with_capacity(capacity: usize) -> Self;
    fn get_or_create_idx(&self, hash: u64) -> usize;
    fn get_strategy_and_idx(&self, hash: u64, out: &mut [f32; K], m: &mut LocalMetrics) -> usize;
    fn get_strategy_into(&self, hash: u64, out: &mut [f32; K]);
    fn get_average_strategy_into(&self, hash: u64, out: &mut [f32; K]);
    fn add_strategy_sum_at(&self, idx: usize, action: usize, prob: f32);
    fn apply_strategy_batch(&self, ops: &mut Vec<StrategyOp>) -> u64;
    fn flush_cpu_batch(&self, batch: &mut Vec<BatchItem>) -> (u64, u64);
    fn snapshot(&self) -> TableSnapshot;
    fn analyze_strategies(&self) -> StrategyAnalysis;
    fn sample_infosets(&self, n: usize) -> Vec<InfoSetDump>;
    fn save_checkpoint(&self, path: &str, iter: u32) -> io::Result<()>;
    fn load_checkpoint(&self, path: &str) -> io::Result<u32>;
}
```

`save_checkpoint` writes format v3 (magic `PKRCKPT3`, i64 strategy_sum).

### 4.3 Trainer

```rust
impl Trainer {
    fn new(abstraction: Arc<dyn AbstractionBuilder>, evaluator: Arc<dyn Evaluator>) -> Self;
    fn with_capacity(abstraction: ..., evaluator: ..., capacity: usize) -> Self;
    fn run_iterations_parallel(&mut self, n: usize);
    fn iteration(&self) -> u32;
    fn is_near_capacity(&self) -> bool;
    fn save_checkpoint(&self, path: &str) -> io::Result<()>;
    fn load_checkpoint(&self, path: &str) -> io::Result<()>;
}
```

### 4.4 Metrics

`pkr-cfr::metrics` provides:

- `LocalMetrics`: plain u64 counters, no atomics, passed by `&mut` through
  the traversal. Each rayon chunk gets its own.
- `GlobalMetrics`: atomics updated once per batch on the coordinator.
- `Snapshot`: point-in-time view with `delta()` for rolling windows.

Counters tracked: nodes, depth sum, max depth, per-depth histogram, cache
hits/misses, infosets created, strategy ops pushed, regret ops pushed,
strategy ops applied, regret ops input/unique, batches, iterations, wall
time, traverse time, merge time, flush time.

### 4.5 Abstraction

See `arch-overview.md` §3.5. Flat indices:

| Street | Combinatoric space | Entries | Bucket type |
|---|---|---|---|
| Preflop | C(52,2) | 1,326 | u8 |
| Flop | C(52,5) × 10 | 25,989,600 | u8 |
| Turn | C(52,6) × 15 | 305,377,800 | u8 |
| River | C(52,5) (board) + hand bucket | 2,598,960 | u8 |

---

## 5. Runtime pipeline

### 5.1 Blueprint format

See `arch-overview.md` §4.1.

### 5.2 Lookup

See `arch-overview.md` §4.2. Binary search over sorted u64 keys; K CDF
bytes per key.

### 5.3 Integration model

`pkr-runtime` is a Rust library. Host applications integrate by:

1. Linking `pkr-runtime` as a dependency (Rust).
2. Cross-compiling to a C ABI (FFI, not currently implemented).
3. Running a small wrapper binary that speaks to the host over a Unix
   socket (not currently implemented).

Only option 1 exists today. FFI and Unix-socket wrappers are on the
roadmap if needed.

---

## 6. Architecture Decision Records (ADRs)

### ADR-001: Canonical DCFR only, no configurable discount

- **Status:** Accepted (2026-09-22)
- **Context:** An earlier `RatioPower` discount formula produced NaN
  around t=3000. The Kuhn harness proved it overflows f32.
- **Decision:** The `DiscountMode` enum has exactly two variants: `None`
  and `CanonicalDcfr`. `RatioPower` was removed. `update_regret_pfr_plus`
  (production) hardcodes canonical.
- **Consequences:** Nobody can select a broken formula. Adding a new
  variant requires a Kuhn harness run proving it is finite and converges.

### ADR-002: Batched parallel training

- **Status:** Accepted
- **Context:** Earlier per-iteration sync made 8 threads slower than 1.
- **Decision:** `run_iterations_parallel(n)` with `ITERS_PER_SYNC=256`.
- **Consequences:** First positive parallel scaling. Flush is now ~30% of
  wall at 8 threads.

### ADR-003: Sort-dedup flush

- **Status:** Accepted
- **Context:** Per-batch HashMap dedup caused malloc jitter.
- **Decision:** `par_sort_unstable_by_key` then walk contiguous groups;
  parallel apply over group ranges.
- **Consequences:** Cheaper than hashing, cache-friendly on the apply
  phase, no per-call map allocation.

### ADR-004: Interleaved regret+momentum, separate strategy_sum

- **Status:** Accepted
- **Context:** False sharing if all three fields are interleaved.
- **Decision:** Regret+momentum interleaved (written serially by
  coordinator); strategy_sum in its own array (written atomically from
  every thread).
- **Consequences:** No false sharing on the parallel-write path.

### ADR-005: i64 strategy_sum

- **Status:** Accepted
- **Context:** i32 fixed-point at scale 1000 saturates after ~2.1M
  weighted visits.
- **Decision:** i64 with the same scale.
- **Consequences:** Costs 240 MB at 5M capacity. Checkpoint format bumped
  to v3.

### ADR-006: Runtime is a library, not a server

- **Status:** Accepted
- **Context:** ASR-005 mandates local API, and 1 ms p99 rules out network
  hop.
- **Decision:** `pkr-runtime` exposes `MmapReader` + `SolverHandle`. No
  network crate linked.
- **Consequences:** Host app provides the transport.

### ADR-007: FMph built but not used at runtime

- **Status:** Accepted (deferred)
- **Context:** Binary search over sorted keys is already < 1 µs; FMph
  would give O(1) at the cost of a more complex writer and reader.
- **Decision:** Keep FMph in `pkr-export` for future use; runtime uses
  binary search.
- **Consequences:** Either wire FMph in or remove it from the writer in a
  future cleanup.

---

## 7. Traceability

| ASR | Solution | Verification |
|---|---|---|
| ASR-001 | Binary search + K-byte read | `pkr-runtime::roundtrip` |
| ASR-002 | mmap'd ~10 MB file | file size |
| ASR-003 | 50M capacity → ~4.8 GB | `capacity() * (2*4 + 6*8) bytes` |
| ASR-004 | 27,700 it/s at 8 threads | `proftest.sh` metrics.csv |
| ASR-005 | No network crate in `pkr-runtime/Cargo.toml` | inspection |
| ASR-006 | `cargo check --target x86_64-unknown-linux-gnu` | CI (not yet wired) |
| ASR-007 | `GameRules` trait exists | `pkr-core::rules` |
