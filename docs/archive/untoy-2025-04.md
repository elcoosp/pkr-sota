# Honest Assessment: Your "Trainer" Is Not a Poker Solver

I've read every file. What you have is a **toy CFR loop on a fake game** wearing a poker costume. Here's exactly what's broken and what's required to make it real.

---

## 🔴 CRITICAL BUGS (these make the entire pipeline meaningless)

### 1. `traversal.rs` uses action history AS the board
```rust
let board = history;   // ← line in traverse(), history.len() >= 2 branch
let raw = evaluator.evaluate_hand(hole, board) as f32;
```
`history` is a `Vec<u8>` of action indices (0,1,2,3). You are feeding action IDs to the evaluator as "community cards." The `TrivialEvaluator` ignores them and returns 0, so this bug is masked. The moment you swap in `NlheEvaluator`, it will panic on `assert_eq!(board.len(), 5)`.

### 2. Terminal condition `history.len() >= 2` is 2-ply
You terminate the game tree after **two actions total**. Real NLHE has ~4 betting rounds × multiple actions each. There is no flop, no turn, no river, no showdown logic. This is closer to matching pennies than poker.

### 3. `Trainer::run_iteration` has no opponent hand, no board
```rust
trainer.run_iteration(&hole, &mut rng);  // one hole, no opp, no board
```
CFR over poker requires sampling opponent hole cards AND chance outcomes (flop/turn/river). You have neither. The "showdown" payoff is `±1` regardless of pot, stack, or actual hand strength.

### 4. `TrivialEvaluator` returns 0 always
```rust
impl Evaluator for TrivialEvaluator {
    fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u16 { 0 }
}
```
Every "showdown" is a tie. CFR has nothing to learn. The trainer produces a uniform strategy forever. **Your blueprint is literally the midpoint of `u8` for every infoset.**

### 5. `NlheEvaluator` is unusable in its current form
- Builds a 2,598,960-entry `HashMap<u64, u16>` lazily on first call → ~150 MB RAM, seconds of startup
- Then evaluates a 7-card hand by trying all **21 five-card combos** with a hash lookup each
- Hard-asserts `board.len() == 5` → unusable preflop/flop/turn
- Rough estimate: **50–100M evals/sec slower than needed** by 100–1000×.

A real evaluator (Cactus Kev + perfect hash, or TwoPlusTwo 32MB table, or PHEvaluator) does 7-card eval in **~10ns with zero startup**.

### 6. `calculate_ehs` is called inside `get_infoset_hash` at runtime
```rust
fn get_infoset_hash(&self, hole, board, _history) -> u64 {
    let (ehs, ehs_sq) = crate::ehs::calculate_ehs(hole, board, self.evaluator.as_ref());
    ...
}
```
`calculate_ehs` runs **1000 Monte Carlo samples** = ~21,000 hand evaluations per call. During CFR traversal, `get_infoset_hash` is called at every node visited. Even with the toy 2-ply game, that's ~84,000 evaluations per iteration. On a real game tree you'd be at **10⁷–10⁸ evals per iteration**. You will not complete a single real iteration in a human lifetime.

### 7. `u8` midpoint-128 regret storage is catastrophically lossy
- Positive regret range: 0–127 (7 bits)
- Negative regret range: 0–128
- DCFR discounting multiplies by factors in [0,1]; under repeated discounting, regrets decay toward 128 and information is destroyed
- `add_regret` uses `saturating_add` so any delta > 127 in one step is clipped
- Real solvers use `f32` or `i32` regrets and quantize only at blueprint export time

### 8. `write_blueprint` builds the MPH on dummy keys
```rust
let keys: Vec<u64> = (0..capacity as u64).collect();
write_blueprint(output_path, trainer.get_table(), &keys);
```
You index the table with `infoset_idx = hash % capacity` during training, but write the MPH using sequential integers `0..capacity`. At runtime, looking up a real `infoset_hash` returns a `Some(...)` that maps to the wrong CDF row. The blueprint is **functionally garbage** for runtime lookup.

### 9. No average strategy tracking
DCFR's output is the **average strategy** over iterations, not the current strategy. You only store regrets. `get_strategy` returns the current regret-matched strategy. The exported blueprint is therefore not even the right object — it should be `Σ_t π_t^σ / T`.

### 10. `NlheRuleset::max_actions_per_node = 4`
No bet sizing. No distinction between check and call. No distinction between bet sizes. "All-in" is one of four actions with no stack context. This cannot represent NLHE.

---

## 🟡 ARCHITECTURAL GAPS (what's missing entirely)

| Component | Status | What's needed |
|---|---|---|
| **Game state** | ❌ none | Pot, stacks, blinds, street, active player, board, hole cards, action history with sizing |
| **Game tree** | ❌ toy 2-ply | Recursive betting rounds: preflop→flop→turn→river→showdown with fold/check/call/bet(all sizes) |
| **Chance nodes** | ❌ none | Sample opponent hole (1 of C(50,2)=1225) + sample flop/turn/river from blocked deck |
| **Legal actions** | ❌ constant 4 | Function of (street, pot, stack, last aggressor, position) |
| **Payoffs** | ❌ ±1 | Pot-aware: fold → win current pot; showdown → equity × pot |
| **Bet abstraction** | ❌ none | Discrete bucket of sizes (e.g., 0.33, 0.5, 0.75, 1.0, 1.5, 2.0, all-in) per street |
| **Preflop abstraction** | ❌ none | 169 canonical (CHP) or OCHS buckets |
| **Postflop abstraction** | ❌ none | Pre-computed k-means on (EHS, EHS², P_{1,2,3}) per street; **stored on disk**, not recomputed |
| **Opponent modeling** | ❌ none | External-sampling MCCFR (sample opp hand + their actions on each traversal) |
| **Average strategy accumulator** | ❌ none | `strategy_sum: Vec<f32>` updated every iteration with reach probability |
| **Parallelism** | ❌ none | Rayon across independent iteration seeds, then merge |
| **Checkpointing** | ❌ none | Save/load regrets + strategy_sum to resume training |
| **Runtime lookup** | ⚠️ broken | Fix MPH to be built on real encountered hashes; add action-translation table |

---

## ✅ WHAT'S REQUIRED — PRIORITIZED ROADMAP

### Phase 0 — Stop lying to yourself (1 day)
Replace `TrivialEvaluator` with a real one. Replace `FastAbstraction` with `KMeansAbstraction` using **precomputed centroids from disk**. Delete `run_iteration` and the entire `traversal.rs`. It cannot be salvaged.

### Phase 1 — Real hand evaluator (2–3 days)
- Implement a **constant-time 7-card evaluator**. Options in priority order:
  1. **PHEvaluator** (C++ port): ~5ns/eval, 10MB table, no startup cost
  2. **Cactus Kev + perfect hash** (lookup2): ~50ns/eval, ~10MB table
  3. **TwoPlusTwo**: ~3ns/eval, 32MB table, hand-rank-by-index
- Remove the 21-combo brute force. Remove the 2.6M-entry `HashMap`. Remove the `assert_eq!(board.len(), 5)`. Accept 5–7 cards and pad with sentinels or special-case 5/6/7 inputs.
- Add a `partial_evaluate(hole, board)` that completes the board via MC for partial-board contexts (only used during abstraction precomputation).
- Target: **≥100M hands/sec single-threaded**. Anything less and CFR is hopeless.

### Phase 2 — Game state + game tree (3–5 days)
Define:
```rust
struct GameState {
    hole: [[u8;2]; 2],         // [hero, villian]
    board: Vec<u8>,            // 0..5
    pot: f32,
    stacks: [f32; 2],
    street: Street,            // Preflop|Flop|Turn|River
    actor: usize,
    history: Vec<Action>,      // (player, kind, size)
    folded: [bool; 2],
}
struct Action { kind: Fold|Check|Call|Bet(f32)|AllIn }
```
Implement:
- `legal_actions(&state) -> Vec<Action>` — drives the tree branching
- `apply_action(&state, a) -> State` — transition
- `is_terminal(&state)` — folded or showdown-after-river
- `terminal_payoff(&state, player) -> f32` — pot-aware
- `deal_chance(&state, rng) -> State` — flop/turn/river sampling with blocking

### Phase 3 — CFR with proper chance sampling (3–4 days)
- **External-sampling MCCFR**: at chance nodes, sample once and recurse. At opponent nodes, sample one action by current strategy.
- Store **both** regret sum (`f32`) and strategy sum (`f32`) per (infoset, action).
- Replace `u8` midpoint table with `Vec<f32>`. Quantize only at export time.
- Proper DCFR: track `t`, apply α=1.5/β=0/γ=2ᵗ/²⁄(t²·α+β·γ) per Brown & Sandholm 2019.
- Update **average strategy** with reach probability every iteration.

### Phase 4 — Abstraction precomputation (2–3 days)
- **Preflop**: 169 canonical buckets via suit-isomorphism (CHP), or finer ~1000 buckets via EHS quantiles
- **Postflop**: For each street, sample N=10k–100k (hole, board) pairs, compute features `(EHS, EHS², P₁, P₂)` over k=200 MC samples, run k-means with k=200 (flop), 200 (turn), 200 (river). **Save centroids to disk.** `KMeansAbstraction` loads from disk and does nearest-centroid in O(k) — no MC at runtime.
- Total budget: ~10⁸ evaluations to build abstractions. Run **once**.

### Phase 5 — Trainer (1–2 days)
- CLI: `--iterations N --abstraction abs.bin --output bp.bin --threads T`
- Each thread runs external-sampling MCCFR with independent RNG and **private** regret table; merge periodically (sum regrets, average strategies).
- Print exploitability estimate every K iterations against best-response (or just print average strategy entropy as a sanity check).
- Checkpoint every M minutes.

### Phase 6 — Real blueprint export (1 day)
- Collect actual `(infoset_hash, average_strategy)` pairs encountered during training (use a `HashMap<u64, [f32; K]>` to dedupe).
- Build MPH on the **real hashes**, not `0..capacity`.
- Quantize CDF to u8 with proper rounding.
- Write action translation table (bet sizes → abstract buckets).

### Phase 7 — Runtime lookup that actually works (1 day)
- Fix `SolverHandle` to use the real MPH (currently the layout in `pkr-export/writer.rs` doesn't match `pkr-runtime/mmap.rs` — `FmphHeader` vs `FmphDataPacked` are two different structs and the runtime only knows about `FmphHeader`).
- Validate end-to-end: train a small Leduc-style subgame, export, mmap, query a known infoset, assert CDF matches.

---

## 📊 CONCRETE NUMBERS TO HIT

| Metric | Toy (current) | Real minimum |
|---|---|---|
| Hand eval speed | ~1M/s (HashMap lookup ×21) | **≥100M/s** |
| Infoset hash lookup | 21k evals (1000 MC × 21) | **≤100ns** (disk-loaded centroids) |
| Regret precision | 8-bit | **32-bit float** |
| Iterations/sec (single thread) | ~1k (toy tree) | **≥10k** (real tree, external sampling) |
| Total infosets trained | 1024 (dummy) | **≥10⁶** (real abstraction) |
| Training time for usable blueprint | ∞ | **≥48 CPU-hours** (200 MTTs of external sampling) |
| Blueprint size | ~1 KB | **50–500 MB** |

---

## TL;DR — What is required

You need to **delete `traversal.rs` and `main.rs`'s toy glue code**, then build, in order:

1. A **fast 7-card evaluator** (≥100M/s)
2. A **real NLHE game state + game tree** with pot/stack/blinds/betting rounds
3. **External-sampling MCCFR** with `f32` regrets + average-strategy accumulator
4. **Pre-computed abstractions** (EHS k-means on disk, not in the hot loop)
5. A **real trainer binary** with parallel threads + checkpointing
6. A **fixed blueprint writer** that builds MPH on actual infoset hashes
7. A **fixed runtime** that round-trips a lookup correctly

Everything you currently have — `pkr-export`, `pkr-runtime`, `pkr-contracts`, `pkr-core` — is reusable as scaffolding. `pkr-cfr`, `pkr-eval`, `pkr-abstraction`, and `pkr-trainer` need substantial rewrites. The MPH infrastructure in `pkr-export/fmph.rs` is fine; it's just being fed garbage keys.

Stop adding tests to the toy. Pick phase 1 and start.
