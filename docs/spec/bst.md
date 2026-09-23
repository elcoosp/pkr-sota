# Behavioral Specification & Test Verification — pkr-sota

| Field | Value |
|-------|-------|
| **Project** | pkr-sota |
| **Document** | Behavioral Specification & Test Verification (Level 4) |
| **Version** | 2.0 |
| **Date** | 2026-09-22 |
| **Status** | Current |

This document describes the verified behavior of the codebase as
implemented. For the aspirational original, see `docs/archive/`.

---

## 1. Behavioral specifications

### 1.1 Pipeline end-to-end

```gherkin
Feature: Training pipeline
  The whole pipeline from precompute to runtime query is verified by
  ./smoke.sh and ./proftest.sh.

  Scenario: Smoke test verifies the pipeline
    Given a fresh checkout
    When I run ./smoke.sh
    Then hand_ranks.bin, centroids.bin, preflop_abstraction.bin,
         flop_abstraction.bin, turn_abstraction.bin, river_buckets.bin
         are created in .smoke/
    And pkr-trainer runs 10 iterations and writes blueprint.bin
    And blueprint.bin loads through pkr-runtime and queries succeed
    And the exit code is 0

  Scenario: Proftest verifies production scale
    Given a fresh checkout
    When I run ./proftest.sh
    Then abstraction tables are created in .proftest/
    And pkr-trainer runs 100K iterations at 8 threads
    And .proftest/metrics.csv has one row per report interval
    And .proftest/stats.json has snapshot + strategy_analysis + samples
    And no row reports nonfinite regrets
    And the exit code is 0
```

### 1.2 Runtime blueprint lookup

```gherkin
Feature: Runtime blueprint lookup
  pkr-runtime is a library. The host app loads it and queries directly.

  Scenario: Load a valid blueprint
    Given a blueprint.bin produced by pkr-trainer
    When MmapReader::new is called
    Then the FileHeader magic is "PKRSOTA1"
    And the version is >= 2
    And the hash_algo is FNV-1a 64-bit (2)
    And mmap succeeds without allocating the file into RAM

  Scenario: Query a known infoset
    Given a SolverHandle wrapping a loaded blueprint
    And infoset_hash is a key present in the blueprint
    When get_advice_fast(infoset_hash) is called
    Then the result is Some(SotaAdvice)
    And the CDF is monotonic non-decreasing
    And the last CDF byte is 255
    And the lookup takes < 1 ms

  Scenario: Query an unknown infoset
    Given a SolverHandle wrapping a loaded blueprint
    And infoset_hash is not in the blueprint
    When get_advice_fast(infoset_hash) is called
    Then the result is None
    And no panic occurs

  Scenario: Blueprint from a different hash_algo is rejected
    Given a blueprint.bin with hash_algo = 1 (legacy DefaultHasher)
    When MmapReader::new is called
    Then the result is Err(MmapError::InvalidHashAlgo)
    And no silent corruption occurs
```

### 1.3 CFR algorithm behavior

```gherkin
Feature: CFR training
  Training runs external-sampling CFR with canonical DCFR and PCFR+
  momentum.

  Scenario: Regrets remain bounded
    Given training runs for 100K iterations at any configuration
    When the run completes
    Then .proftest/stats.json reports nonfinite_count = 0
    And max_abs_regret is finite

  Scenario: Capacity overflow stops cleanly
    Given a trainer with capacity N
    And training produces more than 95% of N unique infosets
    When is_near_capacity() returns true
    Then the trainer stops and writes a final checkpoint
    And no silent clumping of new infosets occurs

  Scenario: Capacity panic is loud, not silent
    Given a trainer with capacity N
    And 100% of N unique infosets have been created
    When a new hash is looked up
    Then alloc_idx panics with a message naming the capacity
    And training does not continue with corrupted data

  Scenario: Non-finite regret triggers a warning
    Given a table whose regret update produces a non-finite value
    When flush_cpu_batch processes the update
    Then a warning is printed once naming the iteration
    And the warning is not repeated
```

### 1.4 CFR convergence on Kuhn

```gherkin
Feature: CFR converges on Kuhn poker
  The Kuhn harness in pkr-testgames verifies the CFR algorithm against a
  game with a known Nash equilibrium.

  Scenario: Vanilla CFR converges to Nash
    Given the Kuhn harness with DiscountMode::None, MomentumMode::Off
    When training runs to 3,000,000 iterations
    Then exploitability is < 1e-2
    And the value of the average strategy is within 1e-4 of -1/18

  Scenario: Canonical DCFR converges to Nash
    Given the Kuhn harness with CanonicalDcfr, MomentumMode::Off
    When training runs to 3,000,000 iterations
    Then exploitability is < 1e-2
    And the value of the average strategy is within 1e-4 of -1/18

  Scenario: RatioPower does not exist
    Given the codebase
    When DiscountMode is inspected
    Then there is no RatioPower variant
    And production code cannot select it
```

---

## 2. Test strategy

| Layer | Location | Covers |
|---|---|---|
| Unit | Various `#[cfg(test)]` | Card, Deck, evaluator, dcfr math, translate |
| Integration | `binaries/pkr-trainer/tests/pipeline.rs` | Train → export → load → query |
| Runtime contract | `crates/pkr-runtime/tests/roundtrip.rs` | Blueprint format round-trip |
| Convergence | `crates/pkr-testgames` | CFR algorithm correctness on Kuhn |
| Smoke | `smoke.sh` | Whole chain with real files, fast |
| Profile | `proftest.sh` | Production scale with metrics |

Test counts as of last commit: 98 passing, 1 ignored
(`load_external_blueprint`, needs `PKR_BLUEPRINT` env var).

### 2.1 What is NOT covered

- `pkr-exploit` is not exercised against a real opponent.
- `pkr-fuzz` is not integrated into the training loop.
- `pkr-cfr::gpu` has one parity test but is not on the production path.
- `pkr-cfr::riversolve` has tests but the implementation is not real CFR.
- `pkr-cfr::valuenet` has tests but is not wired into anything.
- No exploitability measurement on NLHE.
- No end-to-end "play a hand" test.

---

## 3. Performance verification

### 3.1 Throughput

Measured via `proftest.sh` (8 threads, 50M capacity, k=64 centroids):

| Iteration | it/s | cache_hit |
|---|---|---|
| 5,120 | 15,196 | 0.584 |
| 51,200 | 21,159 | 0.873 |
| 100,000 | 27,773 | 0.919 |

Steady-state throughput: **~27,700 it/s**. The first window is slower
because the thread-local idx cache is cold; by iteration 100K the cache is
> 92% warm.

### 3.2 Memory

| Configuration | Regret data | Strategy | Hash map | Total |
|---|---|---|---|---|
| 5M capacity | 240 MB | 240 MB | ~80 MB | ~560 MB |
| 50M capacity | 2.4 GB | 2.4 GB | ~800 MB | ~5.6 GB |

The 50M configuration fits in the M1's 16 GB with headroom. The 200M
configuration (needed for 10⁹ iterations) does not.

### 3.3 Runtime lookup

Verified via `crates/pkr-runtime/tests/roundtrip.rs`. Lookup is a binary
search over N sorted u64 keys plus a K-byte read. For N = 10⁶, that is
~20 comparisons. Cost is dominated by the mmap'd page cache hit; typically
sub-microsecond.

No formal p99 benchmark exists yet.

---

## 4. Requirements traceability matrix

| ASR | Test | Evidence |
|---|---|---|
| ASR-001 (p99 < 1 ms) | `pkr-runtime::roundtrip` | manual measurement, no formal benchmark |
| ASR-002 (< 50 MB) | file size + inspection | blueprint.bin is ~7 MB for 470K infosets |
| ASR-003 (< 12 GB) | table size calculation | 50M → ~5.6 GB |
| ASR-004 (10⁷ < 1 hr) | `proftest.sh` extrapolation | 27,700 it/s → 6 min for 10⁷ |
| ASR-005 (local API) | `pkr-runtime/Cargo.toml` | no network crate |
| ASR-006 (x86_64 Linux) | CI (not wired) | manual `cargo check --target` |
| ASR-007 (variant-agnostic) | `GameRules` trait | only `NlheRuleset` implements it |

---

## 5. Living documentation

- `cargo doc --workspace --no-deps` produces API docs.
- `docs/status.md` is the source of truth for what works.
- This document is the source of truth for what is tested.
- `CHANGELOG.md` (root) summarizes major changes.
