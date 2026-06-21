To achieve **zero merge conflicts** across 4 parallel AI agents while maintaining high development velocity, we must move away from simple crate-level splits and adopt a **Modular File-Isolated Wave System**. 

In this system, the project is broken down into highly granular tasks (typically 1-3 files per task). Tasks are grouped into **Waves**. Within a Wave, no two agents touch the same file or even the same module boundary. An agent can only pick a task if all its dependencies in the previous Wave are completed.

Before spawning agents, you (the orchestrator) must create the Git repo, the `Cargo.toml` workspace, and the **Interface Control Document (ICD)** (the `pkr-contracts` crate). 

Here is the master decomposition plan.

---

### Phase 0: The Interface Contract (Orchestrator Task)
**Do this yourself before spawning any agents.**
Create the workspace and a crate named `pkr-contracts`. Define the exact traits and structs. This is the "API" all agents will code against.

**File to create:** `crates/pkr-contracts/src/lib.rs`
```rust
pub trait GameRules: Send + Sync {
    fn max_actions_per_node(&self) -> u8;
    fn deck_size(&self) -> usize;
    fn hand_size(&self) -> usize;
}

pub struct InfoSet {
    pub hash: u64,
    pub valid_actions: Vec<u8>,
}

pub struct SotaAdvice {
    pub cdf_probabilities: Vec<u8>, // 0-255 representing 0.0-1.0
}

pub trait BlueprintProvider {
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice>;
}

pub trait Evaluator {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u16;
}

pub trait AbstractionBuilder {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8]) -> u64;
}
```

---

### Wave 1: Foundation Primitives (4 Parallel Tasks)
**Goal:** Build the basic data structures and math primitives. No agent depends on each other.

| Task ID | Scope | Exclusive File Paths | Agent Guidance |
|---------|-------|----------------------|----------------|
| **W1-T1** | **Card Primitives** | `crates/pkr-core/src/card.rs`<br>`crates/pkr-core/src/deck.rs` | Create `Card`, `Suit`, `Rank` structs. Create a standard 52-card `Deck`. Implement `NlheRuleset` which implements `pkr_contracts::GameRules`. |
| **W1-T2** | **Hand Evaluator** | `crates/pkr-eval/src/lib.rs`<br>`crates/pkr-eval/src/tables.rs` | Implement a fast 7-card hand evaluator. Implement `pkr_contracts::Evaluator`. You may use the Cactus Kev algorithm or a precomputed lookup table approach. |
| **W1-T3** | **Compact CFR Table** | `crates/pkr-cfr/src/table.rs` | Create `CompactRegretTable`. Store regrets as `u8` (midpoint 128). Implement follow-the-leader strategy generation returning `Vec<f32>`. Use `Vec<u8>` for storage. |
| **W1-T4** | **Binary Format Header** | `crates/pkr-export/src/header.rs` | Define the `#[repr(C)]` structs for `blueprint.bin`: `FileHeader`, `FmphHeader`, `TranslationTableHeader`. Use `serde` and `bytemuck` for safe zero-copy casting. |

---

### Wave 2: Isolated Algorithms (4 Parallel Tasks)
**Goal:** Build the heavy algorithms. Agents can code against the traits from Wave 1.

| Task ID | Scope | Exclusive File Paths | Agent Guidance |
|---------|-------|----------------------|----------------|
| **W2-T1** | **DCFR Math** | `crates/pkr-cfr/src/dcfr.rs` | Implement the Discounted CFR update logic. Write a function `update_regret(current: u8, iteration: u32, delta: f32, is_positive: bool) -> u8`. Formula: `t^a / (t^a + 1)`. α=1.5, β=0. |
| **W2-T2** | **EHS Math** | `crates/pkr-abstraction/src/ehs.rs` | Implement Expected Hand Strength and EHS² (variance). Use `pkr_contracts::Evaluator`. Generate random boards and calculate equity. |
| **W2-T3** | **FMph Hash Gen** | `crates/pkr-export/src/fmph.rs` | Implement the algorithm to build a Minimal Perfect Hash from a `Vec<u64>` of infoset keys. Output the FMph state machine arrays. |
| **W2-T4** | **Action Translation** | `crates/pkr-export/src/translate.rs` | Implement the pseudo-harmonic mapping math. Given `lower_action`, `upper_action`, `actual_action`, and `reach_prob`, return the `(u8, u8)` blend probabilities. |

---

### Wave 3: Integration Logic (4 Parallel Tasks)
**Goal:** Wire the isolated algorithms into functional components.

| Task ID | Scope | Exclusive File Paths | Agent Guidance |
|---------|-------|----------------------|----------------|
| **W3-T1** | **Abstraction Clustering** | `crates/pkr-abstraction/src/cluster.rs` | Implement K-Means clustering. Use the `EHS` and `EHS²` features from W2-T2. Output an implementation of `pkr_contracts::AbstractionBuilder`. |
| **W3-T2** | **CFR Traversal** | `crates/pkr-cfr/src/traversal.rs` | Implement the External Sampling MCCFR loop. Use the `CompactRegretTable` (W1-T3) and `dcfr` (W2-T1). Walk the game tree using `pkr_contracts::GameRules`. |
| **W3-T3** | **Blueprint Serializer** | `crates/pkr-export/src/writer.rs` | Write the `blueprint.bin` writer. Combine the `FileHeader` (W1-T4), `FmphHeader` (W2-T3), u8 CDF strategies, and Translation Table (W2-T4) into a single `mmap`-able file. |
| **W3-T4** | **VPS Memory Map Reader** | `crates/pkr-runtime/src/mmap.rs` | Implement the read-only memory mapper. Parse the `FileHeader` (W1-T4). Expose raw pointers to the FMph structure and CDF arrays. |

---

### Wave 4: High-Level APIs & Orchestrators (4 Parallel Tasks)
**Goal:** Finalize the crates and create the executable binaries.

| Task ID | Scope | Exclusive File Paths | Agent Guidance |
|---------|-------|----------------------|----------------|
| **W4-T1** | **Abstraction Crate API** | `crates/pkr-abstraction/src/lib.rs` | Expose the public API for the crate. Re-export `cluster` and `ehs`. Ensure the `AbstractionBuilder` trait is properly implemented and documented. |
| **W4-T2** | **CFR Crate API** | `crates/pkr-cfr/src/lib.rs` | Expose the public API for the trainer. Create a `Trainer::new(rules, abstraction)` struct that wraps the `traversal` and `table` modules. |
| **W4-T3** | **Runtime Lookup Engine** | `crates/pkr-runtime/src/lookup.rs` | Implement the fast-path lookup. Use the `mmap` reader (W3-T4), execute the FMph hash, and return `pkr_contracts::SotaAdvice`. Implement `pkr_contracts::BlueprintProvider`. |
| **W4-T4** | **Trainer Binary** | `binaries/trainer/src/main.rs` | The orchestrator binary. Initialize NLHE rules, Abstraction, and CFR Trainer. Run for 1,000,000 iterations. Call the exporter to save `blueprint.bin`. |

---

### How to Execute this with 4 AI Agents

1. **Start Wave 1:** Give each of the 4 agents one Task from Wave 1. Provide them with the code inside `pkr-contracts/src/lib.rs` so they know the types they must return/use.
2. **Merge Wave 1:** Once all 4 agents finish, review their code, merge them into the `main` branch. Resolve any trivial import path issues (there shouldn't be any if they followed file paths).
3. **Start Wave 2:** Pull the updated `main` branch. Give each agent a Task from Wave 2. They can now `use pkr_core::*` and `use pkr_contracts::*` because Wave 1 is merged.
4. **Continue Waves:** Repeat this process. Because agents in the same wave never touch the same file, and they only pull dependencies from previous waves, you will experience **zero merge conflicts**.

### Example Prompt for an Agent (W3-T4)
> "You are working on the `pkr-sota` Rust workspace. Your task is **W3-T4: VPS Memory Map Reader**. 
> You must ONLY create or modify the file `crates/pkr-runtime/src/mmap.rs`. Do not touch any other files.
> The workspace already contains `pkr-contracts` and `pkr-export` (which contains `header.rs` with the `FileHeader` struct). 
> Write a struct `MmapReader` that takes a file path, opens it read-only, maps it into memory using the `memmap2` crate, and parses the `FileHeader`. Provide methods to return raw byte slices for the FMph state machine and the CDF strategy array. Ensure all error handling uses `thiserror`."
