# pkr-sota 🃏⚡

A state-of-the-art (SOTA) poker analysis engine for No-Limit Texas Hold'em (NLHE), architected from the ground up to split computation across a **Mac Mini M1 (16GB)** for offline training and a **5€ VPS** for sub-millisecond runtime inference.

Built in Rust (Edition 2024), `pkr-sota` leverages Apple Silicon's unique hardware (AMX coprocessor, unified memory) to train deep-stack blueprints, and exports them into a hyper-compressed, memory-mapped format that runs on the cheapest cloud servers without bundling a network server.

---

## 🏛️ Architecture Overview

The system is strictly split into two environments, connected by a versioned binary artifact (`blueprint.bin`).

### 1. Offline Training (Mac Mini M1)
The expensive computation is offloaded to the M1, exploiting its unified memory and matrix coprocessor to achieve SOTA convergence times.
*   **Algorithm:** Discounted CFR (DCFR) with External Sampling MCCFR for rapid convergence (2-10x faster than vanilla CFR).
*   **Memory Optimization:** Compact CFR using `u8` quantized regrets (follow-the-leader strategy), reducing training memory by 16x to fit within 16GB.
*   **Abstraction:** EHS² (variance proxy) and OCHS (Opponent Cluster Hand Strength). The heavy `equity_vs_cluster` matrix math is offloaded to the M1's **AMX (Apple Matrix Coprocessor)** via `amx-rs`, yielding a 10-30x speedup over NEON.
*   **Depth-Limited Solving:** DeepStack-style depth-limited solving using an **MLX**-trained value network to predict leaf values without computing the entire game tree.

### 2. Runtime Inference (5€ VPS)
The VPS runtime is stripped of all training logic, network layers, and floating-point math. It is embedded directly into your existing poker application via FFI.
*   **Artifact Format:** `blueprint.bin` is a ~10MB memory-mapped file consisting of an FMph (Finite State Machine Minimal Perfect Hash) structure and CDF (Cumulative Distribution Function) quantized to `u8`.
*   **Performance:** O(1), zero-collision, branch-light lookups. The entire blueprint fits in L2/L3 cache, yielding < 1ms p99 latency.
*   **Action Translation:** Implements the pseudo-harmonic mapping (Ganzfried & Sandholm, 2013) via a precomputed table to handle off-tree user bets without breaking Nash equilibrium guarantees.
*   **Memory:** Total runtime RSS footprint is < 50MB, leaving plenty of RAM for your host application on a 2-vCPU / 2-4GB VPS.

---

## 📊 Performance Targets (ASRs)

| Metric | Target | Hardware |
|--------|--------|----------|
| Runtime p99 Lookup Latency | < 1 ms | 5€ VPS (2 shared vCPUs) |
| Runtime Memory Footprint | < 50 MB | 5€ VPS (2-4 GB RAM) |
| Training Memory Footprint | < 12 GB | Mac Mini M1 (16 GB Unified) |
| 6-max Blueprint Convergence | < 7 days | Mac Mini M1 (4 P-cores) |
| Abstraction Equity Speedup | 10-30x via AMX | Mac Mini M1 |

---

## 📦 Workspace Structure

The project is divided into granular, trait-bound crates to allow parallel AI development with zero merge conflicts.

```text
pkr-sota/
├── crates/
│   ├── pkr-contracts/     # Universal traits (API boundary for all crates)
│   ├── pkr-core/          # Card primitives, Deck, GameRules implementations
│   ├── pkr-eval/          # Fast 7-card hand evaluator (Lookup tables)
│   ├── pkr-abstraction/   # EHS² + OCHS math, AMX acceleration, K-Means clustering
│   ├── pkr-cfr/           # DCFR algorithm, Compact CFR table, Tree traversal
│   ├── pkr-export/        # FMph generation, CDF quantization, blueprint serializer
│   └── pkr-runtime/       # VPS memory-map reader, FMph lookup, Action translation
├── binaries/
│   └── pkr-trainer/       # CLI binary to run training loops and export blueprints
└── docs/
    └── tasks/             # Machine-readable task definitions for AI agents
```

---

## 🚀 Usage

### Training a Blueprint (Mac Mini M1)
Run the trainer binary to execute the CFR loop and export the compressed `blueprint.bin`.

```bash
# Run for 1,000,000 iterations and export to artifacts/nlhe.bin
cargo run --release --bin pkr-trainer -- \
    --iterations 1000000 \
    --output artifacts/nlhe.bin \
    --variant nlhe
```

### Integrating the Runtime (5€ VPS)
Because `pkr-sota` does not bundle a WebSocket server, you import it as a library in your existing Rust/Node/Go backend.

```rust
use pkr_runtime::SolverHandle;
use pkr_contracts::{Variant, BlueprintProvider};

fn main() {
    // 1. Initialize the solver (mmap's the blueprint)
    let solver = SolverHandle::new(Variant::NLHE, "./artifacts/nlhe.bin").unwrap();
    
    // 2. Query advice in microseconds
    let infoset_hash = 1234567890; // Calculate this based on your game state
    if let Some(advice) = solver.get_advice_fast(infoset_hash) {
        println!("Fold: {}%, Call: {}%, Raise: {}%", 
            advice.cdf_probabilities[0], 
            advice.cdf_probabilities[1], 
            advice.cdf_probabilities[2]
        );
    }
}
```

---

## 🤖 AI-Driven Development Workflow

This project is built using a **Wave-based Parallel AI Agent Strategy**. 
1. Tasks are defined in `docs/tasks/` with strict file path boundaries.
2. The `generate-agent-prompt.sh` script reads a task, injects workspace context and inter-crate dependencies, and generates a highly constrained prompt.
3. 4 AI agents work simultaneously on separate crates. They code against the `pkr-contracts` traits, ensuring zero merge conflicts upon integration.
4. Quality gates (`cargo nextest run`, `cargo clippy -- -D warnings`) are enforced in every agent's script before commits.

To generate a prompt for a specific task:
```bash
./generate-agent-prompt.sh W1-T1
```

---

## 🛠️ Tech Stack

*   **Language:** Rust (Edition 2024)
*   **Apple Silicon:** `amx-rs` (Matrix Coprocessor), MLX (Value Network Training)
*   **Memory:** `memmap2` (Zero-copy runtime), `bytemuck` (Pod/Zeroable casting)
*   **Concurrency:** `rayon` (Parallel abstraction math), `rand` (MCCFR sampling)
*   **Logging/CLI:** `tracing`, `clap`

## 📄 License
TBD (Proprietary / Open Source - configure as needed)
