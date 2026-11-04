# pkr-sota: Architecture

**Last verified against code:** 2026-11-03 (docs synthesis). This document
describes the architecture *as implemented*. For current status and measured
numbers see [`STATUS.md`](STATUS.md); for the experiment record see
[`HISTORY.md`](HISTORY.md).

---

## 1. Constraint model

| Dimension | Training host | Runtime host |
|---|---|---|
| Role | Precompute abstractions + train blueprint | Frozen lookups |
| Constraints | Must not exceed ~12 GB resident | Small resident set; cheap VPS or embedded |
| Artifact | `blueprint.bin` (self-describing) | `mmap` of `blueprint.bin` |

Two consequences drive everything:

1. **Train offline, query online.** All CFR logic, precompute binaries, and
   mmap'd abstraction tables live on the training side. The runtime artifact
   is the *only* interface between the two halves: the runtime host does not
   need the training sources to serve queries. (Note: the `pkr-runtime`
   *crate* does link `pkr-eval`, `pkr-abstraction`, `pkr-cfr`, and
   `pkr-subgame` — river search and evaluation happen on the runtime host
   inside the query process. What the runtime does not do is *train*.)
2. **The artifact is self-describing and O(log n).** A binary search over
   sorted `u64` keys with mmap'd CDF bytes; no allocations, no locks on the
   hot path.

---

## 2. Pipeline

```text
                 training host (offline)
┌──────────────────────────────────────────────────────────────┐
│  pkr-abstraction-precompute                                  │
│    hand_ranks.bin · centroids · per-street bucket tables     │
│                         │                                    │
│                         ▼                                    │
│  pkr-trainer ── pkr-cfr  (external-sampling MCCFR + DCFR)    │
│                         │                                    │
│                         ▼                                    │
│  pkr-export ──▶ blueprint.bin  (sorted keys + u8 CDFs)       │
└──────────────────────────────────────────────────────────────┘
                 runtime host (online)
┌──────────────────────────────────────────────────────────────┐
│  pkr-runtime                                                 │
│    mmap → branchless binary search → advice                  │
│    RuntimeSession: range tracking · off-tree bet translation │
│    river-only subgame search (bounded · CFR-D gadget)        │
└──────────────────────────────────────────────────────────────┘
```

---

## 3. Training side

### 3.1 CFR algorithm

`pkr-cfr` implements external-sampling CFR with:

- **Regret matching**: at each infoset the current strategy is the positive
  part of accumulated regrets, renormalized; uniform when all regrets are
  non-positive.
- **Discounted CFR** (canonical, Brown & Sandholm 2019): regrets are
  discounted by `t^p / (t^p + 1)` for `t ≥ τ`; `PKR_DCFR_ALPHA=1.5`,
  `β = 0`, `γ = 2` by default.
- **PCFR+ momentum** (Farina, Kroer, Sandholm 2021) is optional
  (`PKR_MOMENTUM`) and off by default; `PKR_AVG_POWER=p` weights the
  strategy-sum averaging by `t^p` (0 = uniform, 2 = DCFR γ).
- **Regret updates are overflow-safe** (`dcfr::update_regret_i64` uses
  saturating adds and an exact `u64` discount; no-op during warmup).
- **Average strategy**: own-reach-weighted sum in `i64` fixed point.

### 3.2 CompactRegretTable

Regrets live in a lock-free concurrent map onto flat atomic planes: 112
bytes per infoset at K=7. Strategy sums are written atomically from every
thread; regret/momentum planes are written by the batch coordinator only,
which removes false sharing. Thread-local lookup caches front the map on
the traversal hot path.

### 3.3 Traversal and batching

`Trainer::run_iterations_parallel(n)`:

1. Reserve `n` iteration numbers with one atomic op; split into chunks.
2. Rayon work-steals chunks across threads; each runs into thread-local
   buffers (regret deltas and strategy ops are *accumulated*, not applied
   during traversal).
3. Merge all buffers on the coordinator: `apply_strategy_batch` (sorted
   groups → strategy sum) and `flush_cpu_batch` (sorted groups → regret
   update), both parallel over unique keys.
4. Fold per-thread metrics into global.

Batch size is `--iters-per-sync` (default 2048 — the v36 capacity sweep
showed ~20% more throughput than 512 with no measurable convergence cost).

### 3.4 Action abstraction (K=7)

Every bet is bucketed into one of 7 discrete actions (fold, check/call,
four pot-relative bet sizings [0.5, 1, 2, 3]×pot, jam). The mapping is
defined once in `pkr-core/src/abstraction.rs`, and the CI gate
`ci/scripts/check-no-hardcoded-k.sh` rejects any hard-coded copy of the
bucket count. `NUM_ACTION_BUCKETS` changed 6 → 7 on 2026-10-10
(checkpoint-invalidating by design; K=6 and K=7 are different abstract
games). A strict NLHE min-raise variant (correct
short-all-in-does-not-reopen-betting rule) is opt-in via
`PKR_STRICT_MIN_RAISE=1` and bumps `action_legal_v` so strict and loose
checkpoints cannot cross-load.

### 3.5 Card abstraction

`pkr-abstraction` maps `(hole, board, history, street)` → infoset hash:

- **Preflop**: the exact 1,326 strategic classes (rich 6D centroid model
  by default).
- **Flop**: `(combinadic_rank_5, hole_mask_index)` → 25,989,600-entry
  table.
- **Turn**: `(combinadic_rank_6, hole_mask_index)` → 305,377,800-entry
  table (305 MB).
- **River**: exact hand rank `>> RIVER_TIER_SHIFT` (shift = 15, ~287 hand
  tiers) combined with a board bucket.

Tables are built by k-means over equity features (MC EHS/EHS² and equity
histograms compared with earth-mover distance). A missing table is a hard
error in production paths unless `PKR_ALLOW_EHS_FALLBACK=1` (slow
Monte-Carlo fallback; its use is counted and logged). The stack depth is
fingerprinted too (`stack_bb_div2`): a 100bb checkpoint refuses to load
into a 200bb run.

### 3.6 Fingerprinting and determinism

A 40-byte `AbstractionFingerprint` describing every abstraction choice
(buckets, shifts, sizings, stack depth, action-legal version, …) is
written into checkpoints and blueprints; loaders reject mismatches loudly
instead of silently mis-hashing. Same seed + inputs → byte-identical
`blueprint.bin` at any thread count, enforced by the `golden` CI workflow
against stored SHA-256 hashes. Checkpointing is rolling and atomic
(`.tmp → .prev → final`) with a streaming loader (1 MB chunks, ~1 MB peak
transient even at 60M infosets).

---

## 4. Runtime side

`pkr-runtime` is a library plus three binaries (`pkr-bot-pipe` line
protocol, `health` latency probe, examples for head-to-head play).

### 4.1 Artifact format

`blueprint.bin` (little-endian), version **v4** — the only version the
writer emits; the reader accepts v2–v4 and rejects anything newer loudly:

```text
[FileHeader:32]      magic "PKRSOTA1" · version · variant_id ·
                     infoset_count · max_actions_k · hash_algo(2=FNV-1a-64)
[AnchorsSection:64]  per-street bet-sizing anchors
[Fingerprint:40]     AbstractionFingerprint (v4 only)
[key_count:u32][cdf_size:u32]
[keys: key_count × u64]   ascending
[cdf:  cdf_size × u8]     K bytes per key, monotonic
```

Writing is atomic (`.tmp` + rename), keys are sorted defensively, and the
quantized u8 CDF always closes at 255 on the last non-zero-probability
action. The reader validates magic, version, hash algorithm, count
consistency (`cdf == keys × k`) and the fingerprint before trusting the
file. An optional FMph minimal-perfect-hash tail can be emitted
(`PKR_EMIT_FMPH=1`) for a future O(1) path; the runtime binary-searches
regardless.

### 4.2 Lookup

`SolverHandle::get_advice_fast(hash)`: branchless binary search over the
sorted keys, read K CDF bytes, return `SotaAdvice { cdf_probabilities,
len }` — no allocations, no atomics, bounds-checked. On a miss a fixed
mild fallback policy is returned and counted, never garbage. Measured
p50 ≈ 42 ns / p99 ≈ 84 ns on a large blueprint (see STATUS.md).

### 4.3 Session layer

Above the blueprint sits `RuntimeSession`:

- **Opponent range tracking** — a 1326-hand posterior updated from
  observed actions; since Phase 4.3 the tracker uses the *search* policy
  (when a kept-whole search root produced the action), not the blueprint.
- **Shadow state** (`shadow.rs`) — an in-tree view advanced by
  *translated* actions, used for blueprint hashes and the tracker.
- **Off-tree bet translation** — nearest in-tree bucket via pseudo-harmonic
  translation (Ganzfried & Sandholm 2013).
- **River-only subgame search** inside a wall-clock budget
  (`PKR_VSEARCH_BUDGET_MS`, default 2000). Two safe modes:
  - **Bound** (`--search-bound`; live: `PKR_VSEARCH=1 PKR_VSEARCH_BOUND=1`)
    — solve, measure the opponent's best response, and blend back toward
    the blueprint until that BR is no better than against the blueprint.
    The bound (`final_br ≤ blueprint_br`) is proven by test; the deadline
    is enforced inside BR walks.
  - **CFR-D gadget** (Burch et al. 2014) — a terminate option paying
    blueprint continuation value (EV, not BR — see HISTORY.md for the bug
    story). Proven safe in sampled and vector form.

Turn search costs ~45 s per decision and stays offline; river decisions
fit the live budget (~0.7 s).

---

## 5. Crate boundaries

```text
pkr-contracts (no deps) ── Evaluator · AbstractionBuilder ·
                            BlueprintProvider · fnv1a infoset hash
     ▲
pkr-core ── Card, Deck, GameState, NLHE rules, K=7 action abstraction
     ▲
pkr-eval ── reference slow eval · C(52,5) mmap table · Fast7Evaluator
            (bit-identical, differential-tested)
     ▲
pkr-abstraction ── EHS/EHS² · histograms · k-means · precompute bin
     ▲
pkr-cfr ── Trainer · CompactRegretTable · DCFR · valuenet · riversolve
     ├──▶ pkr-export ── blueprint writer/reader (v4, fingerprints)
     ├──▶ pkr-subgame ── vector solver (arena, no unsafe aliasing)
     │                    · RangeTracker · CFR-D gadget
     └──▶ pkr-runtime ── MmapReader · SolverHandle · RuntimeSession
                          · vsearch · shadow · pkr-bot-pipe / health
pkr-exploit ── sampled BR (held-out LB) · myopic LBR · public BR · spot eval
pkr-fuzz ── differential rules fuzzing · duplicate-poker tournament/arena
pkr-testgames ── Kuhn exact-BR harness (Nash −1/18)
pkr-leduc-check ── Leduc convergence CI canary
binaries/pkr-trainer ── the orchestrating CLI (~42 flags)
```

`pkr-exploit` is wired into the trainer's eval path and the `expl_eval` /
`lbr_eval` binaries; `pkr-fuzz` runs in CI (`weekly` fuzz jobs) and its
tournament/arena harnesses are the H2H measurement path. `pkr-testgames`
and `pkr-leduc-check` are convergence canaries over the production CFR
path.

---

## 6. Test strategy

| Layer | What | Where |
|---|---|---|
| Unit | cards, deck, eval, DCFR, translate, … | per-crate `#[cfg(test)]` (~480 tests) |
| Integration | pipeline end-to-end | `binaries/pkr-trainer/tests/`, `crates/*/tests/` |
| Golden | deterministic training | `ci/golden-training.sha256` via `golden` workflow |
| Runtime contract | blueprint roundtrip, loader fuzzing | `crates/pkr-runtime/tests/`, `crates/pkr-fuzz` |
| Smoke | whole pipeline on real files | `smoke.sh` (+ CI `smoke` workflow) |
| Convergence | Kuhn (exact BR), Leduc gate (< 70 mchips) | `pkr-testgames`, `pkr-leduc-check` |
| Unsafe review | Miri on core crates | `weekly` workflow |
| Exploitability | sampled BR / LBR instruments | `pkr-exploit`, `expl_eval`, `lbr_eval` |
