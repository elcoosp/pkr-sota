# pkr-sota → Winning the chipzen.ai Arena (M1 16GB-feasible roadmap)

**Companion to:** `pkr-sota-3-bug-fix-plan.md` (the 3 code bugs — assume those land first).
**Constraint:** all training on Mac Mini M1, 16 GB unified memory. All items below are sized to that box.

---

## 0. Where the prize is actually won (read this first)

Arena EV decomposes into three layers, and their leverage is **not** proportional to how "research-grade" they sound:

| Layer | What it decides | Typical EV share in bot arenas |
|---|---|---|
| **Coverage & robustness** | Do you play *anywhere near* sane on every street, every hand, without timing out or panicking? | Losing here = losing the tournament outright |
| **Exploitation vs. weak bots** | Most entrants are scripted baselines / under-trained blueprints. Crushing *them* is where leaderboard margin comes from | The actual prize margin |
| **Blueprint ε-exploitability** | Matters only vs. the 2–3 other strong bots | Decides top-2 vs top-1 |

So the plan below is ordered: **fix coverage → add exploitation → then buy blueprint quality with the M1 time you have left.** A slightly-worse blueprint + a working exploit layer + zero crashes beats a marginally-tighter blueprint that plays uniform-random on the river.

---

## 1. P0 — Foundation (before any training run)

These are already covered in detail in the fix plan — listed here as dependencies only:

- [ ] **Bug 3** — stable infoset hashing (FNV-1a + `hash_algo` header guard + golden vectors)
- [ ] **Bug 2** — solve through the river (turn/river must exist in the blueprint at all)
- [ ] **Bug 1** — touched-only GPU flush (without it you cannot afford enough iterations on the M1)
- [ ] Decide the `opponent_reach` weighting question while you're in `traverse` (noted at the end of the fix plan)

Everything in sections 2–5 assumes these.

---

## 2. P1 — Blueprint quality upgrades that fit the M1 budget

### 2.1 DCFR discounting (cheapest big win, ~1 day)

You already planned DCFR; wire it into the flush path. Concrete parameters from Brown & Sandholm 2019: **α = 1.5, β = 0, γ = 2, τ = 1000** — regret arrays are multiplied by `(t/τ)^α` (for t < τ), strategy-sum by `(t/τ)^β` (floored near ε), global iteration weight `(t/τ)^γ`. Your `cpu_momentums` array suggests the shader already does RM+-style cumulation — keep that; discounting composes with RM+ (that's the DeepStack/Slumbot-grade combination). Expected effect: 2–10× convergence for ~30 lines of WGSL/CPU code.

### 2.2 Raise river abstraction resolution (cheap, high ROI)

k = 200 clusters per street is the weakest exactly where pots are biggest. River equity is nearly deterministic (no more cards to come), so river clustering is your *cheapest* resolution upgrade:

- River: k = 200 → **1000–2000**. At river you can use *exact* made-hand strength from the evaluator (no MC needed) — clustering cost is one `evaluate_hand` per hand, trivially parallel on the M1.
- Turn: k = 200 → 400–500 if memory allows (see §6 budget).
- Flop/preflop: leave at 200; preflop is chart-dominated anyway (§5.3).

Storage cost of the river lookup table scales with k; verify the flat-index table still fits your mmap design.

### 2.3 Action abstraction + translation table (must actually ship)

The review flagged that the export ships a *zero-length placeholder* for the translation table. That must be filled, because the runtime currently maps abstract buckets to concrete sizes arbitrarily:

- Keep your 6 buckets (fold / call / <0.5×pot / <1×pot / >1×pot / all-in), but **generate the concrete sizes geometrically per street** (e.g. 0.45× / 0.9× / 2.2× pot, plus jam) so every bucket has a well-defined anchor.
- Implement the **pseudo-harmonic mapping** (Ganzfried & Sandholm) for off-tree opponent sizes — you specced `compute_translation` in the export steps; this is its concrete purpose. Without it, a 0.7×-pot opponent bet lands in a bucket whose "response" was trained for 0.5× — silent strategy corruption on every off-size bet, i.e. *most of them*.

### 2.4 One round of iterated refinement (if timeline allows)

After training v1: probe 20–30 canonical spots with a local best-response on a truncated tree, find which streets leak the most, add clusters / refine sizes *there*, retrain once. One iteration of this ≈ Slumbot-style incremental gains. Do not attempt more than one round — diminishing returns vs. your calendar.

### 2.5 What NOT to do on the M1

- No full-scale ReBeL/PSNF-style public-belief training — memory and time budget explode.
- No potential-aware / N-level abstraction research — engineering risk with no deadline-friendly payoff.
- Don't chase exploitability below ~50 mbb/g in the abstract game; the exploit layer (§3) matters more at the arena.

---

## 3. P0 — Exploitation overlay (the actual prize lever)

This is the layer your current repo doesn't have at all, and it's where the leaderboard margin vs. a field of scripted/weak bots comes from. Design principle: **stay within ±10–15% of the blueprint** so you never become the exploitable one.

### 3.1 Per-opponent stat tracking (stateless arena-safe)

Maintain decay-weighted histograms, keyed by opponent id (whatever the SDK exposes), reset never, decay λ tuned to expected hand count per opponent:

- fold-to-bet per street (vs. 1 size bucket), fold-to-raise, check-call vs check-raise freq
- aggression freq, WTSD, avg bet size / pot
- call-vs-all-in frequency and realized showdown hands → range-shape evidence

Each update is O(1); the whole layer is a few KB of state and zero latency.

### 3.2 Bounded best-response shifts

Turn stats into policy nudges *at the infoset level*:

- Opponent folds too much vs. bets (fold-freq > blueprint's assumption + margin) → raise your bluff ratio at that node class by `min(δ, 0.5 × excess_fold)`, exactly the direction the bluff-indifference condition prescribes.
- Opponent over-calls → thin value up, drop bluffs.
- Opponent jams too wide → call jam with exact hand-vs-range equity (your evaluator already does this exactly — see 3.4).

Hard cap every deviation at **±12%** absolute probability; re-center toward blueprint every N hands (anti-adaptation); disable shifts in pots > 120 bb unless the stat has ≥ 50 supporting samples. This keeps worst-case loss bounded while printing EV vs. Stations and Nits, which is 70% of any arena field.

### 3.3 Where Jev (typesafe.ai) fits — and where it doesn't

Consistent with the earlier assessment: Jev is a **meta-layer accelerator, not the strategy core**. Concrete slot: its typed Choice/Score decision model classifies the opponent into 4–6 archetypes (Station / Nit / Aggro / Balanced / Randomizer) from the action stream at negligible latency, and the archetype selects/weights an exploit profile from a small precomputed set. Keep it **behind a trait with a hand-rolled EWMA fallback**, and never on the critical path of `Bot.decide` — if the Jev integration is down, the bot must not notice. Ship the fallback first; integrate Jev only if the harness is green.

### 3.4 Exact all-in response (prints EV vs. weak bots, half a day)

Many arena bots over-shove. For any shove facing decision, skip the blueprint and compute **exact hand-vs-range equity** with the evaluator over the opponent's modeled range (preflop: 169-bucket weights; postflop: cluster-weighted ranges). Compare to pot odds. This single rule beats over-shoving bots at near-theoretical rates and costs microseconds.

---

## 4. P0 — Serving & robustness (don't lose on technicalities)

- **Latency discipline:** mmap'd blueprint + FMph + u8 CDF lookup; target p99 < 100 ms against the 5 s budget. Watchdog: if any decision path exceeds a 1 s soft budget, answer from a canned fallback (preflop chart for preflop, pot-odds call/fold heuristic postflop). A timeout is a forfeited hand — never let a panic, a NaN, or a slow first mmap page-in cost you one.
- **Rules fuzzing (1–2 days, non-negotiable):** HU edge cases: min-raise legality and all-in-below-min-raise (does it reopen action?), uncalled-bet return, split pots, exact stack arithmetic. Differential-test `GameState` against a reference implementation over 10⁵ random action sequences. Rule bugs look like "bad luck" and silently bleed EV for the whole tournament.
- **Numerical guards:** epsilon in every normalization; clamp regret/strategy reads; never divide by zero pot (`max(1.0)` pattern you already use — replicate everywhere).
- **Eval harness:** 10k-hand matches vs. 3 scripted archetypes (station / nit / aggro) + any public baseline bot you can wire in; track mbbs/hand and action-distribution-per-street (uniform distribution on turn/river = Bug 2/3 regression alarm). Run it in CI on every change.

---

## 5. P2 — Stretch goals (only if the calendar is green)

### 5.1 Runtime river re-solve (no neural net needed)

At river start, the board is complete and your evaluator is exact — so a depth-limited CFR re-solve over the river subgame (opponent range from blueprint reach probabilities, leaves = exact showdown) is ~1700 hand-combos × K actions × ~200 iterations ≈ 10⁷ ops → **milliseconds in Rust**, comfortably inside 5 s on the VPS. This gives you near-solver river play *without training anything*, sidestepping river abstraction error entirely. This is the highest-ceiling stretch item.

### 5.2 MLX value network for turn leaves (only after 5.1 proves out)

Your existing roadmap item — train a small MLP (EHS²/OCHS features → value) on the M1, export ONNX, use as leaf values for turn re-solves. Slot it into the same call-site as the river roll-out.

### 5.3 Preflop chart validation

HU preflop is publicly well-mapped; eyeball your trained preflop strategy against known ranges (BU open ~80%+, BB defend very wide, 3-bet ~13–16%). A structurally wrong preflop chart bleeds EV every single hand and is the most common arena-bot failure. Half a day, zero risk.

---

## 6. M1 16GB feasibility math (why all of this fits)

| Component | Size |
|---|---|
| CompactRegretTable: 3 arrays × 5M infosets × 6 actions × 4 B | 360 MB |
| DashMap `hash_to_idx` (~5M u64→usize) | ~150–250 MB |
| GPU persistent buffers (regrets + momentums) | 240 MB |
| Abstraction tables (preflop 1326 + flop/turn flat + river k≤2000) | ~50–150 MB |
| OS + Rust toolchain + benchmarks headroom | ~2 GB |
| **Total steady-state** | **~1.0–1.3 GB of 16 GB** |

You have >10× headroom — enough for the river/turn cluster expansion (§2.2) and comfortable checkpointing (snapshot the table as a flat mmap dump every 30–60 min so a crash never costs a training weekend).

Throughput after the Bug-1 fix: the flush path drops from ~120M atomic ops/iteration to O(batch); the bottleneck returns to traversal. With 8 P-cores traversing and DCFR converging 2–10× faster, a meaningful HU blueprint at your abstraction size is a **1–3 day training run**, with checkpoints — not a research project.

---

## 7. Priority board (do in this order)

| Pri | Item | Effort | EV impact |
|---|---|---|---|
| P0 | 3 bug fixes (fix plan) | 4–8 h | Enables everything |
| P0 | Robustness harness: watchdog + fallback + rules fuzzing + eval matches | 2–3 d | Protects the whole tournament |
| P0 | Exploitation overlay: stats + bounded shifts + exact all-in calls | 2–3 d | **Prize margin vs. weak bots** |
| P1 | DCFR discounting + geometric sizes + pseudo-harmonic translation shipped in export | 1–2 d | Blueprint quality ×2–10 |
| P1 | River/turn cluster expansion + clean retrain (1–3 d run) | 0.5 d + run | Blueprint quality |
| P2 | Runtime river re-solve | 3–5 d | Near-solver river play |
| P2 | Preflop chart sanity + MLX value net + Jev meta-layer (trait-guarded) | 1–3 d | Polish / meta edge |

**Sequencing rule:** never start a long training run until P0 exploitation code exists but *is disabled* during training — it only activates at serve time.
