# Behavioral Specification & Test Verification — pkr-sota

| Field | Value |
|-------|-------|
| **Project** | pkr-sota |
| **Document** | Behavioral Specification & Test Verification (Level 4) |
| **Version** | 1.1 |
| **Date** | 2025-04-21 |
| **Author** | Lead Architect (assisted by AI) |
| **Status** | Accepted |

---

## 1. Behavioral Specifications (BDD Scenarios)

The following Specification by Example (SbE) scenarios define the executable acceptance criteria for the `pkr-sota` Rust library. They are written in Gherkin syntax, ready to be automated using `cucumber-rust` or `rspec`. 

### 1.1 Feature: Runtime Blueprint Loading & Lookup
**Context:** `pkr-runtime` is integrated as a standard Rust crate (library) within the host poker application. No FFI, no Unix sockets—just direct Rust function calls.

```gherkin
Feature: Runtime Blueprint Loading and Fast Lookup
  As a poker application developer
  I want to load a memory-mapped blueprint and query it in microseconds
  So that I can provide real-time analysis to my users via my own WebSocket server

  Scenario: Successfully loading a variant blueprint
    Given a valid "nlhe_blue.bin" file exists at "./artifacts/"
    When the host app calls `SolverHandle::new(Variant::NLHE, "./artifacts/nlhe_blue.bin")`
    Then the solver should memory-map the file read-only
    And the solver should parse the FMph header and variant metadata
    And the resident memory footprint should increase by less than 15MB

  Scenario: Fast-path infoset lookup (Happy Path)
    Given the solver is initialized with a NLHE blueprint
    When the host app calls `get_advice_fast(infoset_hash)` for a valid preflop hash
    Then the solver should execute an FMph hash lookup
    And the solver should return an `SotaAdvice` struct containing CDF u8 probabilities
    And the execution time should be less than 1 millisecond

  Scenario: Handling unknown infoset hashes (Unwanted Behavior)
    Given the solver is initialized with a NLHE blueprint
    When the host app calls `get_advice_fast(infoset_hash)` with a hash not in the blueprint
    Then the solver should return `None` (or a uniform fallback strategy)
    And the solver must NOT panic or crash
```

### 1.2 Feature: Pseudo-Harmonic Action Translation
**Context:** Users frequently bet off-tree sizes (e.g., 42% pot). The solver must map these to the trained discrete actions (e.g., 33% and 50%) using pseudo-harmonic mapping to remain unexploitable.

```gherkin
  Scenario Outline: Translating an off-tree bet size
    Given the solver is initialized with a NLHE blueprint
    And the valid abstract actions at the current node are 0.33 and 0.50 pot
    And the precomputed reach probabilities favor 0.33
    When the user bets "<Bet Fraction>" of the pot
    Then the solver should query the precomputed translation table
    And it should return a blended `SotaAdvice` using the pseudo-harmonic formula
    But it should NOT use nearest-neighbor mapping

    Examples:
      | Bet Fraction |
      | 0.42         |
      | 0.38         |
      | 0.49         |
```

### 1.3 Feature: Variant-Agnostic Trait Boundary
**Context:** ASR-007 requires the core engine to support multiple variants. This validates the `GameRules` trait abstraction.

```gherkin
  Scenario: Initializing a non-NLHE variant (Future Proofing)
    Given a `PloRuleset` struct implements the `GameRules` trait
    When the trainer is instantiated with `Trainer::new(PloRuleset)`
    Then the Compact CFR table should allocate memory based on PLO's max action count (e.g., 6)
    And the AMX equity calculator should use a 52-card deck and 4-card hand evaluations
```

---

## 2. Test Strategy & Plan

The test strategy follows the "Test Pyramid" (or "Testing Trophy") philosophy, heavily favoring fast, deterministic unit and integration tests in pure Rust, as the network layer is handled by the host application.

### 2.1 Test Matrix

| Test Layer | Scope / Goal | Tools / Frameworks | Relation to Specs |
|---|---|---|---|
| **Unit Tests** | Core logic: DCFR math, CDF encoding/decoding, u8 quantization, AMX matrix kernels. | Rust `#[test]`, `proptest` (property-based) | Validates ASR-003 (Memory), ASR-004 (DCFR logic) |
| **Integration Tests** | Blueprint export/import, FMph collision checks, `pkr-runtime` lookup latency. | Rust `#[test]`, `criterion` (benchmarks) | Validates ASR-001 (Performance), ASR-002 (Memory) |
| **Contract Tests** | Ensure `blueprint.bin` format backward compatibility. | Custom Rust snapshots | Validates ASR-007 (Extensibility) |
| **Property-Based** | Verify CFR invariants (e.g., regrets + strategy sum = avg strategy). | `proptest` crate | Validates ASR-004 (Algorithm correctness) |

### 2.2 Unit Testing Focus: Compact CFR & DCFR
Property-based testing is critical here to ensure the DCFR discounting math (`t^alpha / (t^alpha + 1)`) never results in NaN or overflow when applied to `u8` quantized regrets.
- **Property 1:** For any `u8` regret array, converting to strategy via follow-the-leader must yield a valid CDF where the last element is always 255.
- **Property 2:** DCFR positive regret discounting must strictly decrease the magnitude of older iterations.

---

## 3. NFR Verification Plans (Performance & Memory)

Since the core ASRs revolve around extreme hardware constraints, performance and memory tests are first-class citizens and must be automated in CI.

### 3.1 Performance Verification (ASR-001, ASR-004)

**Test: Runtime Lookup Latency (p99 < 1ms)**
- **Environment:** Simulated 2-vCPU x86_64 Linux environment (Docker container limiting CPU).
- **Tool:** `criterion` Rust crate.
- **Procedure:**
  1. Load `nlhe_blue.bin` via `mmap`.
  2. Generate 10,000 random valid infoset hashes.
  3. Execute `get_advice_fast(hash)` in a tight loop.
- **Pass Criterion:** p99 latency < 1,000 microseconds (1ms). p50 latency < 500 microseconds.

**Test: AMX Equity Acceleration (Mac M1)**
- **Environment:** Mac Mini M1.
- **Tool:** `criterion`.
- **Procedure:** Benchmark `equity_vs_cluster_amx` against a scalar NEON fallback.
- **Pass Criterion:** AMX implementation must be at least 10x faster than the NEON baseline for 16x16 fp32 matrix multiply.

### 3.2 Memory Verification (ASR-002, ASR-003)

**Test: VPS Memory Footprint (< 50MB)**
- **Environment:** Linux VPS.
- **Tool:** `/usr/bin/time -v` or `pmap`.
- **Procedure:** Run a dummy host application that links `pkr-runtime` and loads the NLHE and PLO blueprints.
- **Pass Criterion:** Total RSS (Resident Set Size) of the solver component must not exceed 50MB.

**Test: Training Memory Footprint (< 12GB on M1)**
- **Environment:** Mac Mini M1 (16GB).
- **Tool:** macOS Activity Monitor / `vmmap`.
- **Procedure:** Initialize the 6-max NLHE trainer with 1.3M infosets.
- **Pass Criterion:** `pkr-cfr` memory allocation must remain under 12GB, proving the Compact CFR (u8) quantization is effective.

---

## 4. Requirements Traceability Matrix (RTM)

This matrix traces the Architecturally Significant Requirements (ASRs) from the Level 3 spec through to the behavioral scenarios and test plans defined in this document.

| ASR ID | Requirement | Behavioral Scenario / Test | Verification Method | Evidence |
|--------|-------------|----------------------------|---------------------|----------|
| **ASR-001** | Runtime p99 < 1ms | Scenario: Fast-path infoset lookup | Test (Performance via `criterion`) | CI Benchmark Report |
| **ASR-002** | Runtime RAM < 50MB | Scenario: Successfully loading a variant blueprint | Test (Memory via `pmap`) | CI Memory Profile |
| **ASR-003** | Training RAM < 12GB | Test: Training Memory Footprint | Test (Memory via `vmmap`) | Local M1 Validation Log |
| **ASR-004** | Training < 7 days | Test: AMX Equity Acceleration | Test (Performance via `criterion`) | CI Benchmark Report |
| **ASR-005** | Local API only | Scenario: Successfully loading a variant blueprint | Inspection (Code review ensures no network deps in `Cargo.toml`) | `cargo tree` output |
| **ASR-006** | x86_64 Linux portable | All Integration Tests | Test (CI pipeline on Linux runner) | CI Green Build |
| **ASR-007** | Variant-Agnostic | Scenario: Initializing a non-NLHE variant | Test (Unit/Compile test) | Rust compiler success for mock `TestGame` variant |

---

## 5. Living Documentation Strategy

Because `pkr-sota` is a high-performance library rather than a user-facing app, the "living documentation" will be maintained via:

1. **Rustdoc:** The primary source of truth for API usage. The `pkr-runtime` crate will have front-page documentation with a clear example of how a host app initializes the solver and calls `get_advice_fast`.
2. **CI Artifacts:** `criterion` benchmark reports and memory profiling outputs will be published as CI artifacts on every merge to `main`, providing a historical trend of performance and memory usage.
3. **ADRs:** Architecture Decision Records (from L3 spec) will live in `docs/adr/` within the repository, linked directly from the Rustdoc for major architectural components (e.g., the `CompactRegretTable` struct will link to ADR-001).
