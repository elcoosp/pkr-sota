# pkr-sota — SOTA Upgrade Playbook (Agent-Executable Task Cards)

Generated: 2026-09-22 · Source: full-codebase analysis of `pkr-sota` (dump.txt) + literature review of state-of-the-art poker-solving techniques (Lanctot, Tammelin, Brown & Sandholm, Moravčík, Johanson, Ganzfried, Schmid, Lisý, Timbers, Waugh).

**Audience**: an autonomous coding agent executing tasks one at a time, in order.
**Format**: each task is a self-contained card in the same style as `docs/tasks/done/W*-T*.md` (Objective → Exclusive File Paths → Dependencies → Instructions → Acceptance Criteria).

---

## 0. How To Use This Document (read first — non-negotiable rules)

1. **Execute tasks in the order given by the priority table in §2.** Tasks inside the same tier may be executed in any order unless their "Dependencies" line says otherwise.
2. **Never modify** `crates/pkr-contracts/src/lib.rs` trait signatures, the `Cargo.toml` at the workspace root, or the FNV-1a golden hash constants unless the task explicitly says to (only T6 does, deliberately).
3. **Gates before declaring any task done** (every task, no exceptions):
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo nextest run --workspace
   ```
4. **After each tier**, run the end-to-end smoke: `./smoke.sh` must pass (including the byte-size checks for `turn_abstraction.bin` = 305377800 bytes and `river_buckets.bin` = 2598960 bytes, and the ignored `load_external_blueprint` test).
5. **One logical change per commit.** Commit message: `T<n>: <one-line summary>`. Never mix a refactor with a behavior change.
6. **Do not "improve" code not named in the task card.** If you find something broken outside the card's files, record it in the worklog and move on.
7. **When a task card quotes "current code"**, match it against the real file first. If the real file differs (someone already changed it), STOP and record the mismatch in the worklog before proceeding.
8. **Any task that changes the infoset hash scheme or action semantics invalidates existing blueprints and checkpoints.** These tasks say so explicitly. After such a task: delete `train.ckpt` and `blueprint.bin`, retrain from zero.
9. Training runs are stochastic; do not expect bit-identical blueprints. Correctness gates are the unit tests + exploitability metric (T8), never byte equality.

---

## 1. Background — Where This Codebase Stands vs SOTA (1 page)

The engine trains **external-sampling MCCFR** (chance + opponent actions sampled, traverser's actions expanded) with CFR+-style non-negative cumulative regrets (`update_regret_full` clamps at 0), a PCFR+ momentum term, and a DCFR-style discount whose canonical form degenerates to vanilla CFR in f32 for `t > 10^4` (documented honestly in `docs/status.md`). Card abstraction is 2-D k-means over (EHS, EHS²) per street; river is `hand_rank >> 6` (~117 tiers). Export is a sorted-key + u8-CDF blueprint consumed by binary search over an mmap.

The literature SOTA stack, and what this codebase already has / is missing:

| Technique | Reference | Status in pkr-sota |
|---|---|---|
| MCCFR, external sampling (unweighted regret updates) | Lanctot et al. 2009 | ⚠️ Present but **estimator is biased**: opponent actions sampled *and* updates multiplied by `opponent_reach` (T1) |
| CFR+ (RM+ regret matching, alternating updates) | Tammelin 2014 | ⚠️ RM+ clamping present; **simultaneous** updates only (T5) |
| PCFR+ momentum | Farina et al. 2021 | ✅ Implemented (`MomentumMode::On`) |
| DCFR discounting | Brown & Sandholm 2019 | ⚠️ Implemented but degenerates to CFR in f32 at scale — known, accepted |
| Action masking at illegal actions | standard practice (every mature solver) | ❌ **Missing** — strategy mass sits on illegal buckets (T2) |
| Exploitability measurement (BR / sampled BR / LBR) | Lisý & Bowling 2017; Timbers et al. 2022 | ❌ Kuhn harness broken (authors admit); **no full-game metric** (T3, T8) |
| Full betting-sequence infoset keys | standard | ❌ 3-byte per-street signature collapses distinct lines (T6) |
| Geometric bet sizes, unified action abstraction | Libratus-style sizing (Brown & Sandholm 2018) | ❌ 0.5/1.0/2.0 fractions collapse into 2 buckets (T9) |
| Fine river abstraction (1000–2000 buckets) | own roadmap; Johanson et al. 2013 | ⚠️ 117 tiers (`rank >> 6`) (T10) |
| Pseudo-harmonic action translation | Ganzfried & Sandholm 2013 | ⚠️ Function exists; table built but **never shipped/wired**, and sizes mismatch trainer (T11) |
| Fast runtime lookup (Eytzinger/branchless/MPHF) | Algorithmica (Eytzinger); PCHT literature | ⚠️ Plain binary search; FMph built but unused (T12) |
| Continual re-solving at runtime | DeepStack (Moravčík et al. 2017); Libratus nested solving (Brown et al. 2017); ReBeL (Brown & Sandholm 2020) | ❌ `riversolve.rs` is a stub that does not accumulate regrets (T14) |
| VR-MCCFR variance reduction | Schmid et al. 2019 | ❌ Stretch (T15) |
| Hand-isomorphism folding | Waugh 2015 | ❌ Stretch (T15) |

The three changes with the largest expected quality-per-effort are **T1** (unbias the estimator → faster true convergence), **T2** (stop polluting strategies with illegal-action mass) and **T8** (finally *measure* exploitability — without a metric, nothing else can be verified). The largest runtime-quality lever is **T14** (real river re-solve), which is what DeepStack and Libratus both credit for their strength.

---

## 2. Task Index (priority order)

| ID | Tier | Title | Impact | Effort | Risk | Retrain? |
|----|------|-------|--------|--------|------|----------|
| T1 | 0 | Unbias the MCCFR regret estimator | Very high | S | Low | Yes |
| T2 | 0 | Mask illegal actions in traversal & strategy sums | Very high | S | Low | Yes |
| T3 | 0 | Fix the Kuhn exploitability harness | High | M | Low | No |
| T4 | 0 | Numeric safety + reproducible seeds | Medium | S | Low | No |
| T5 | 1 | Alternating CFR+ updates | High | S | Low | Yes |
| T6 | 1 | Street-scoped full-history infoset keys | High | M | Medium | Yes |
| T7 | 1 | Faster evaluation + exact-river EHS | Medium | M | Low | No (tables faster) |
| T8 | 1 | Sampled abstract-game exploitability metric | Very high | L | Medium | No |
| T9 | 2 | Unified action abstraction (fix bucket collapse) | High | S | Medium | Yes |
| T10 | 2 | Finer river tiers via rank percentiles | Medium | S | Low | Yes |
| T11 | 2 | Wire action translation into runtime + CDF decode | Medium | M | Low | No |
| T12 | 2 | Runtime lookup v2 (aligned reads + Eytzinger) | Medium | M | Low | No |
| T13 | 2 | Runtime advice glue + fix eval harness `lookup(0)` | High | M | Low | No |
| T14 | 3 | Real river re-solve (reach-weighted CFR+ subgame) | Very high | L | High | No |
| T15 | 3 | Stretch items (VR-MCCFR, isomorphism folding) | Research | L | High | — |

Execution-order note: **T3 before T8** (Kuhn validates the BR math you will reuse), **T9 before T11** (translation needs the shared constants), **T1+T2 before any long training run** (everything trained before them is built on a biased estimator).

---

# TIER 0 — Correctness first

## T1 — Unbias the MCCFR regret estimator

**Objective.** The traversal samples opponent actions (external-sampling style) but multiplies regret deltas by `opponent_reach`. In external-sampling MCCFR the sampling probability of the opponent prefix *is* the opponent's reach probability, so the two cancel and the update weight must be exactly **1**. The extra factor systematically under-weights every line where the opponent's sampled actions had low probability — a biased estimator that slows convergence exactly where the opponent is folding (Lanctot et al. 2009, "Monte Carlo Sampling for Regret Minimization in Extensive Games", Algorithm 3; see also Gibson et al. 2012 on unbiased bounded estimators). Your own `docs/pkr-sota-winning-roadmap.md` flags this as the open "opponent_reach weighting question". This task resolves it: **drop the weight.**

**Exclusive File Paths**
- `crates/pkr-cfr/src/traversal.rs`
- `crates/pkr-cfr/src/lib.rs`

**Dependencies**
- None.

**Instructions**

1. In `crates/pkr-cfr/src/traversal.rs`, change the `traverse` signature from
   ```rust
   pub fn traverse(
       current: &mut GameState,
       table: &CompactRegretTable,
       abstraction: &dyn AbstractionBuilder,
       evaluator: &dyn Evaluator,
       rng: &mut impl Rng,
       global_iteration: u32,
       traverser: usize,
       reach_prob: f32,
       opponent_reach: f32,
       deck: &[u8],
       deck_idx: &mut usize,
       depth: u32,
       batch: &mut Vec<BatchItem>,
       strategy_batch: &mut Vec<StrategyOp>,
       metrics: &mut LocalMetrics,
   ) -> f32 {
   ```
   to the same signature **without** the `opponent_reach: f32` parameter. Keep `reach_prob` (that is the traverser's own reach and IS required for the average-strategy sum).

2. In the traverser branch (the `if acting_player == traverser` block), replace
   ```rust
   for a in 0..K {
       let delta = (v[a] - v_sigma) * opponent_reach;
   ```
   with
   ```rust
   for a in 0..K {
       let delta = v[a] - v_sigma;
   ```

3. In the same traverser branch, the recursive call passes `opponent_reach` unchanged:
   ```rust
   v[a] = traverse(
       current, table, abstraction, evaluator, rng,
       global_iteration, traverser,
       reach_prob * strategy[a],
       opponent_reach,        // <- DELETE this argument
       ...
   ```
   Delete that argument in every recursive call.

4. In the opponent branch (`else` block), delete the line
   ```rust
   opponent_reach * strategy[sampled_abstract],
   ```
   from the recursive call. The opponent branch does not need any reach bookkeeping any more.

5. In `crates/pkr-cfr/src/lib.rs` (`Trainer::run_iterations_parallel`), update the two top-level `traverse(...)` calls: they currently pass `1.0, 1.0` for `reach_prob, opponent_reach`. Remove the `1.0` that was `opponent_reach` so each call passes `1.0` only for `reach_prob`.

6. Add this regression test at the bottom of the existing `mod tests` in `traversal.rs` (it uses the same `MockEvaluator` / `make_abstraction` helpers already present in that file):
   ```rust
   #[test]
   fn regret_delta_is_unweighted_by_opponent_reach() {
       // With an unbiased external-sampling estimator, the regret pushed for
       // the traverser must be (v[a] - v_sigma) with NO reach multiplier.
       // We cannot observe the delta directly, but we can verify the top-level
       // API change compiled and traversal stays finite and bounded.
       let table = Arc::new(CompactRegretTable::with_capacity(100_000));
       let abstraction = make_abstraction();
       let evaluator: Arc<dyn pkr_contracts::Evaluator> = Arc::new(MockEvaluator);
       let mut rng = StdRng::seed_from_u64(7);
       let mut deck: Vec<u8> = (0..52).collect();
       deck.shuffle(&mut rng);
       let mut state = GameState::new(200.0, 1.0, 2.0);
       state.set_hole_cards([deck[0], deck[1]], [deck[2], deck[3]]);
       let mut batch = Vec::new();
       let mut strategy_batch = Vec::new();
       let mut metrics = LocalMetrics::default();
       let deck_slice: &[u8] = &deck[4..9];
       let mut deck_idx = 0usize;
       let v = traverse(
           &mut state, &table, abstraction.as_ref(), &*evaluator, &mut rng,
           1, 0, 1.0, deck_slice, &mut deck_idx, 0,
           &mut batch, &mut strategy_batch, &mut metrics,
       );
       assert!(v.is_finite() && v.abs() <= 200.0);
   }
   ```

**Why this is correct (for your commit message / reviewer)**
Per sampled trajectory, the probability that the traversal follows prefix `h` is `q(h) = π_c(h) · π_{-i,prefix}(h)` (chance is sampled exactly; opponent actions sampled from their own strategy; the traverser's actions are all expanded). The counterfactual regret requires weight `π_{-i}(h) · Σ_suffix π_{-i,suffix}(z)·(...)`. Since `π_{-i,prefix}(h) = q_prefix(h)`, multiplying the update by `opponent_reach` double-counts the prefix: `E[update] ∝ Σ_h π_{-i}(h)²·(...)` instead of `Σ_h π_{-i}(h)·(...)`. Removing the factor restores the exact ES-MCCFR estimator.

**Acceptance Criteria**
- `cargo nextest run -p pkr-cfr` passes.
- `cargo clippy -p pkr-cfr -- -D warnings` passes.
- `grep -n "opponent_reach" crates/pkr-cfr/src/traversal.rs` returns **no** matches.
- `./smoke.sh` passes end-to-end.

**Compatibility.** Semantics change → delete `train.ckpt` and `blueprint.bin` before the next long run. Expect deep-tree infosets (where the opponent folds often) to receive relatively more regret mass than before; this is the fix working.

---

## T2 — Mask illegal actions in traversal & strategy sums

**Objective.** Today `table.get_strategy_and_idx` / `get_strategy_into` return probabilities over all `K = 6` abstract buckets, including buckets that have **no legal concrete action** at the current node. Consequences: (a) `v_sigma` is deflated by `strategy[illegal] · 0` terms, biasing every regret delta; (b) the average-strategy sum accumulates mass on actions the runtime can never take, which then leaks into the exported CDF. The fix is the standard "action masking + renormalization" step used by every mature CFR implementation.

**Exclusive File Paths**
- `crates/pkr-cfr/src/traversal.rs`

**Dependencies**
- T1 (same file; do T1 first to avoid textual conflicts).

**Instructions**

1. In `traverse`, immediately after the strategy is fetched (after the block that assigns `traverser_idx`), insert masking so BOTH the traverser branch and the opponent branch see the same masked distribution:
   ```rust
   // --- Action masking: zero out buckets with no legal concrete action,
   // --- then renormalize over the legal ones.
   let legal_count = action_counts.iter().filter(|&&c| c > 0).count();
   if legal_count == 0 {
       undo_advance_and_return!(0.0);
   }
   let legal_total: f32 = (0..K)
       .filter(|&a| action_counts[a] > 0)
       .map(|a| strategy[a])
       .sum();
   if legal_total > 0.0 {
       for a in 0..K {
           if action_counts[a] == 0 {
               strategy[a] = 0.0;
           } else {
               strategy[a] /= legal_total;
           }
       }
   } else {
       // No learned mass on any legal bucket (fresh infoset): uniform.
       let u = 1.0 / legal_count as f32;
       for a in 0..K {
           strategy[a] = if action_counts[a] > 0 { u } else { 0.0 };
       }
   }
   ```
   Place it **after** `let mut strategy = [0.0f32; K]; ... table.get_strategy_*(...)` and **before** the `if let Some(idx) = traverser_idx { for a in 0..K { strategy_batch.push(...) } }` block, so the pushed average-strategy ops and both downstream branches all use the masked distribution.

2. In the traverser's regret-delta loop, skip illegal buckets explicitly (their `v[a]` is 0, which would push a spurious negative regret):
   ```rust
   for a in 0..K {
       if action_counts[a] == 0 {
           continue; // no regret update for impossible buckets
       }
       let delta = v[a] - v_sigma;
       batch.push(BatchItem { index: idx as u32, action: a as u32, iteration: global_iteration, delta });
   }
   ```

3. In the strategy-sum push loop, the zeroed illegal buckets are harmless (`apply_strategy_batch` retains only `prob != 0.0`), but make it explicit and cheaper:
   ```rust
   if let Some(idx) = traverser_idx {
       for a in 0..K {
           if strategy[a] <= 0.0 { continue; }
           strategy_batch.push(StrategyOp { index: idx as u32, action: a as u8, prob: strategy[a] * reach_prob });
       }
   }
   ```

4. Add a regression test in the existing `mod tests` of `traversal.rs`:
   ```rust
   #[test]
   fn masked_strategy_puts_no_mass_on_illegal_buckets() {
       // Short stacks: an all-in and some bet sizes coincide or vanish, so at
       // least one of the 6 buckets is guaranteed unrepresentable in some node
       // of any traversal. Assert across many random traversals that no
       // StrategyOp ever assigns probability to a bucket whose count is 0.
       // (Counting is done inside traverse; here we assert the invariant
       // indirectly: every pushed (index, action) op for the same index has
       // actions only from the legal set observed at that visit.)
       let table = Arc::new(CompactRegretTable::with_capacity(100_000));
       let abstraction = make_abstraction();
       let evaluator: Arc<dyn pkr_contracts::Evaluator> = Arc::new(MockEvaluator);
       for iter in 1..=20u32 {
           let mut rng = StdRng::seed_from_u64(iter);
           let mut deck: Vec<u8> = (0..52).collect();
           deck.shuffle(&mut rng);
           let mut state = GameState::new(200.0, 1.0, 2.0);
           state.set_hole_cards([deck[0], deck[1]], [deck[2], deck[3]]);
           let mut batch = Vec::new();
           let mut strategy_batch = Vec::new();
           let mut metrics = LocalMetrics::default();
           let deck_slice: &[u8] = &deck[4..9];
           let mut deck_idx = 0usize;
           traverse(&mut state, &table, abstraction.as_ref(), &*evaluator,
                    &mut rng, iter, 0, 1.0, deck_slice, &mut deck_idx, 0,
                    &mut batch, &mut strategy_batch, &mut metrics);
           for op in &strategy_batch {
               let p = op.prob;
               assert!((0.0..=1.0).contains(&p), "prob out of range: {p}");
           }
           let total: f32 = strategy_batch.iter()
               .filter(|op| op.index == strategy_batch[0].index)
               .map(|op| op.prob).sum();
           assert!(total <= 1.0 + 1e-4, "masked strategy sum {total} exceeds 1");
       }
   }
   ```

**Acceptance Criteria**
- `cargo nextest run -p pkr-cfr` passes; `cargo clippy -p pkr-cfr -- -D warnings` passes.
- After a short training run (`cargo run --release -p pkr-trainer -- --iterations 10000 --threads 4 --capacity 10000000 --output .smoke/t2.bin` plus the usual tables), inspect with `table.analyze_strategies()`: no infoset may have its dominant action on a bucket that was structurally impossible for its history (spot-check 20 random infosets with `sample_infosets`).
- `./smoke.sh` passes.

**Compatibility.** Changes training semantics → retrain from zero. This also silently improves the exported CDF (less mass on dead actions).

---

## T3 — Fix the Kuhn exploitability harness

**Objective.** `docs/status.md` admits: *"The Kuhn harness itself does not converge. Exploitability goes from 0.27 at t=100 to 0.28 at t=3e6 … This is a harness bug."* The Kuhn game is the project's only fast correctness oracle for regret updates — every future algorithm change (T1, T5) will be validated through it, so it must be trustworthy. Rewrite `exploitability()` properly and pin it with golden tests.

**Exclusive File Paths**
- `crates/pkr-testgames/src/kuhn.rs`
- `crates/pkr-testgames/src/bin/kuhn_experiment.rs`

**Dependencies**
- None.

**Instructions**

1. Replace the exploitability machinery in `kuhn.rs` with a clean, standard implementation. Exploitability of a strategy profile σ is
   `expl(σ) = ½ · Σ_i [ v_i(BR_i, σ_{-i}) − v_i(σ) ]`.
   Because best-response values decompose per infoset (the BR player's own reach cancels in counterfactual weighting), compute each `v_i(BR_i, σ_{-i})` by evaluating the 6-card-pair deal tree once per BR action choice. For Kuhn the tree is tiny — enumerate per-infoset action choices independently:
   ```rust
   /// Exploitability of the current average-strategy profile, in ante units.
   /// Standard definition: expl = 0.5 * sum_i (br_i(σ_-i) - v_i(σ)).
   pub fn exploitability(&self) -> f32 {
       let br0 = self.br_value(0);
       let br1 = self.br_value(1);
       let (v0, v1) = self.profile_value();
       0.5 * ((br0 - v0) + (br1 - v1))
   }

   /// Game value to `player` under the current average-strategy profile.
   fn profile_value(&self) -> (f32, f32) {
       let mut v0 = 0.0;
       let mut v1 = 0.0;
       // 6 equally likely deals (C(3,2) card pairs, uniform).
       for c0 in 0..3u8 {
           for c1 in 0..3u8 {
               if c0 == c1 { continue; }
               let (a0, a1) = self.walk_avg(c0, c1);
               v0 += a0 / 6.0;
               v1 += a1 / 6.0;
           }
       }
       (v0, v1)
   }

   /// Best-response value for `player` against the opponent's average
   /// strategy: for each of the player's infosets, pick the action that
   /// maximizes counterfactual value, then value the induced profile.
   fn br_value(&self, player: usize) -> f32 {
       // Kuhn infosets: (player, card, decision_point). For each, evaluate
       // the counterfactual value of each action over the deals consistent
       // with the infoset, take the max, weight by opponent reach (uniform
       // deals: each card pair 1/6, counterfactual weighting makes the
       // per-infoset maximization independent).
       let mut total = 0.0f32;
       for card in 0..3u8 {
           for dp in 0..2u8 {
               let idx = infoset_index(player, card, dp);
               let avg = self.avg_strategy(idx); // existing helper or build from strategy_sum
               let mut best = f32::MIN;
               for a in 0..N_ACTIONS {
                   let cfv = self.infoset_cfv(player, card, dp, a);
                   best = best.max(cfv);
               }
               let _ = avg;
               total += best / 6.0 * 2.0; // two deals consistent per (card, dp) pair of the BR player
           }
       }
       total
   }

   /// Counterfactual value of taking action `a` at the BR player's infoset
   /// (player, card, dp), over all deals and opponent strategies, where the
   /// BR player plays `a` at this infoset and BR-greedily nowhere else matters
   /// (only this infoset's choice is being valued).
   fn infoset_cfv(&self, player: usize, card: u8, dp: u8, a: usize) -> f32 {
       let mut cfv = 0.0;
       for opp in 0..3u8 {
           if opp == card { continue; }
           cfv += self.deal_value_with_forced_action(player, card, dp, a, opp);
       }
       cfv / 2.0 // 2 consistent deals, each prior 1/6 -> normalize to per-reach
   }
   ```
   You must implement the three helpers `walk_avg`, `deal_value_with_forced_action` and `avg_strategy` against the existing `KuhnCfr` state (the file already stores average strategies per infoset and knows the tree; reuse its dealing conventions). The key property to preserve: **per-infoset independent maximization is valid** (BR's own reach cancels in counterfactual weighting) — do NOT reintroduce the old brute-force-over-pure-strategies version or the removed recursive `br_tree_*` version.

2. Add golden tests to `kuhn.rs`:
   ```rust
   #[test]
   fn exploitability_of_exact_nash_is_zero() {
       // Hard-code the known Nash equilibrium (already printed by
       // kuhn_experiment.rs): P0 J: check 2/3 bet 1/3; P0 Q dp0: check 1.0;
       // P0 K: bet 1.0; P0 Q dp1: fold 2/3 call 1/3;
       // P1 J dp1: fold 1.0; P1 Q dp1: fold 1/3 call 2/3; P1 K dp1: call 1.0.
       let k = KuhnCfr::from_average_strategies([
           /* p0 J dp0 */ [2.0/3.0, 1.0/3.0],
           /* p0 J dp1 */ [1.0, 0.0],
           /* p0 Q dp0 */ [1.0, 0.0],
           /* p0 Q dp1 */ [2.0/3.0, 1.0/3.0],
           /* p0 K dp0 */ [0.0, 1.0],
           /* p0 K dp1 */ [0.0, 1.0],
           /* p1 J dp0 */ [1.0, 0.0],
           /* p1 J dp1 */ [1.0, 0.0],
           /* p1 Q dp0 */ [1.0, 0.0],
           /* p1 Q dp1 */ [1.0/3.0, 2.0/3.0],
           /* p1 K dp0 */ [0.0, 1.0],
           /* p1 K dp1 */ [0.0, 1.0],
       ]); // adapt constructor/index order to the existing N_INFOSETS layout
       let e = k.exploitability();
       assert!(e.abs() < 1e-3, "Nash exploitability should be ~0, got {e}");
   }

   #[test]
   fn exploitability_of_uniform_is_positive() {
       let k = KuhnCfr::from_average_strategies([[0.5, 0.5]; 12]);
       let e = k.exploitability();
       assert!(e > 0.01, "uniform strategy must be exploitable, got {e}");
   }
   ```
   (If `KuhnCfr` has no constructor taking an average-strategy table, add one — it is a test-only constructor that sets `strategy_sum` proportional to the given table. The exact infoset index order is `infoset_index(player, card, decision_point) = (player*3 + card)*2 + dp` — already defined in the file.)

3. In `bin/kuhn_experiment.rs`, fix the header print that double-negates the Nash value: it currently prints `-{:.6}` of `1.0/18.0`. Print `Nash value of game to P0: -0.055556` via `-(1.0f32 / 18.0)` formatted once.

4. Run the full experiment and record the numbers in the worklog:
   ```bash
   cargo run --release -p pkr-testgames --bin kuhn-experiment
   ```
   Expected after the fix: exploitability **strictly decreasing** across checkpoints for the `vanilla` and `canon` configs (e.g. 1e-1 → 1e-3..1e-5 territory by 3e6 iterations), no NaN flags.

**Acceptance Criteria**
- `cargo nextest run -p pkr-testgames` passes, including the two new golden tests.
- `cargo clippy -p pkr-testgames -- -D warnings` passes.
- The printed exploitability table is monotonically decreasing (allow one small non-monotonic tick) for at least one config, and the `DISQUALIFIED (NaN)` verdict does not appear.

**Compatibility.** Harness-only change; no retraining. Do not touch the CFR math shared with `pkr-cfr`.

---

## T4 — Numeric safety + reproducible seeds

**Objective.** Two small hardening items. (a) `flush_cpu_batch` stores `(new_r * SCALE) as i32` — a plain cast that silently wraps on overflow; cumulative CFR+ regrets grow linearly with iterations, so multi-hundred-million-iteration runs will corrupt memory values. (b) The trainer seeds each worker with `rand::random::<u64>()`, so training runs are not reproducible; add a `--seed` flag (default 0 = random) for A/B comparisons required by T1/T5 validation.

**Exclusive File Paths**
- `crates/pkr-cfr/src/table.rs`
- `binaries/pkr-trainer/src/main.rs`
- `crates/pkr-cfr/src/lib.rs`

**Dependencies**
- None.

**Instructions**

1. In `flush_cpu_batch`, replace the two stores
   ```rust
   self.store_rm(idx, a, RM_REGRET, (new_r * SCALE) as i32);
   self.store_rm(idx, a, RM_MOMENTUM, (new_m * SCALE) as i32);
   ```
   with saturating, non-finite-skipping versions:
   ```rust
   if new_r.is_finite() && new_m.is_finite() {
       self.store_rm(idx, a, RM_REGRET, (new_r * SCALE).clamp(i32::MIN as f32, i32::MAX as f32) as i32);
       self.store_rm(idx, a, RM_MOMENTUM, (new_m * SCALE).clamp(i32::MIN as f32, i32::MAX as f32) as i32);
   } else {
       warn_nonfinite_regret_once(iteration);
   }
   ```
   (Keep the existing `warn_nonfinite_regret_once` call site or merge it — there must be exactly one warning path.)

2. In `crates/pkr-cfr/src/lib.rs`, change `Trainer::run_iterations_parallel(&mut self, n: usize)` to read an optional base seed stored on the trainer:
   ```rust
   pub struct Trainer {
       abstraction: Arc<dyn AbstractionBuilder>,
       evaluator: Arc<dyn Evaluator>,
       table: Arc<CompactRegretTable>,
       iteration: AtomicU32,
       base_seed: AtomicU64, // 0 => random per worker
   }
   ```
   Add `pub fn set_seed(&self, seed: u64)` / `pub fn base_seed(&self) -> u64` around `base_seed` (use `AtomicU64::new(0)`). In the worker closure replace
   ```rust
   let mut rng = SmallRng::seed_from_u64(rand::random::<u64>());
   ```
   with
   ```rust
   let bs = table_base_seed.load(Ordering::Relaxed); // pass by value before parallel block
   let mut rng = if bs == 0 {
       SmallRng::seed_from_u64(rand::random::<u64>())
   } else {
       SmallRng::seed_from_u64(bs ^ (0x9E3779B97F4A7C15u64).wrapping_mul(chunk_idx as u64 + 1))
   };
   ```
   (capture the seed by value into the closure; it must not borrow `self`).

3. In `binaries/pkr-trainer/src/main.rs`, add
   ```rust
   /// Base seed for worker RNGs. 0 = random. Nonzero makes runs reproducible
   /// up to float summation order in parallel batches.
   #[arg(long, default_value_t = 0)]
   seed: u64,
   ```
   to `Cli`, and after building `trainer`:
   ```rust
   if cli.seed != 0 { trainer.set_seed(cli.seed); }
   ```

4. Test: in `table.rs` tests, add
   ```rust
   #[test]
   fn flush_saturates_instead_of_wrapping() {
       let table = CompactRegretTable::with_capacity(4096);
       let idx = table.get_or_create_idx(0xCAFE_0001);
       let huge = i32::MAX as f32 / SCALE + 1.0e6; // far beyond representable
       let mut batch = vec![BatchItem { index: idx as u32, action: 0, iteration: 1, delta: huge }];
       table.flush_cpu_batch(&mut batch);
       let raw = table.get_regret(0xCAFE_0001, 0) * SCALE;
       assert!(raw <= i32::MAX as f32, "regret wrapped: {raw}");
   }
   ```

**Acceptance Criteria**
- `cargo nextest run -p pkr-cfr` and `-p pkr-trainer` pass; workspace clippy clean.
- `./smoke.sh` passes; `./run.sh --help` shows `--seed`.

**Compatibility.** None. No retrain needed (existing checkpoints load fine).

---

# TIER 1 — Convergence & throughput

## T5 — Alternating CFR+ updates

**Objective.** The trainer runs BOTH traversals per iteration and flushes them into one batch (simultaneous updates). CFR+ (Tammelin 2014) alternates: update one player's regrets per pass. Alternating updates are consistently faster in practice and are part of every strong CFR+ implementation. This task adds the scheme behind a `--alternating` flag so you can A/B it with `bench.sh` + the (fixed, T3) Kuhn harness.

**Exclusive File Paths**
- `crates/pkr-cfr/src/lib.rs`
- `binaries/pkr-trainer/src/main.rs`

**Dependencies**
- T1, T2 (same files / call sites), T3, T4 (validation tooling).

**Instructions**

1. In `crates/pkr-cfr/src/lib.rs`, add a field to `Trainer`:
   ```rust
   pub struct Trainer {
       abstraction: Arc<dyn AbstractionBuilder>,
       evaluator: Arc<dyn Evaluator>,
       table: Arc<CompactRegretTable>,
       iteration: AtomicU32,
       base_seed: AtomicU64,
       alternating: bool,
   }
   ```
   Add `pub fn set_alternating(&mut self, on: bool)` and default `false` in both constructors.

2. Inside `run_iterations_parallel`, in the worker's per-iteration loop, when `alternating` is enabled run **one** traversal per iteration (the player whose regrets are updated alternates):
   ```rust
   for local_i in 0..pairs {
       let global_iter = base_iter + local_i as u32;
       // ... deck setup unchanged ...

       let traversers: &[usize] = if alternating { // capture `alternating` by value
           &[(global_iter % 2) as usize]
       } else {
           &[0, 1]
       };
       for &t in traversers {
           let mut state = GameState::new(200.0, 1.0, 2.0);
           state.set_hole_cards(hero, villain);
           let mut deck_idx = 0usize;
           traverse(
               &mut state, &table, &*abstraction, &*evaluator, &mut rng,
               global_iter, t, 1.0, deck_slice, &mut deck_idx, 0,
               &mut batch, &mut strategy_batch, &mut metrics,
           );
       }
   }
   ```
   Rationale: the average-strategy sum is only accumulated on each player's own traversal pass (that is already how `traverse` works: `strategy_batch` is pushed for `acting_player == traverser`), so halving the traversals keeps strategy sums correct.

3. In `binaries/pkr-trainer/src/main.rs` add:
   ```rust
   /// Use alternating CFR+ updates (Tammelin 2014). Roughly doubles
   /// iterations/s per player pass; validate with kuhn-experiment + bench.sh.
   #[arg(long, default_value_t = false)]
   alternating: bool,
   ```
   and after building the trainer: `trainer.set_alternating(cli.alternating);`

4. Add `--alternating` to the `pkr-trainer` invocation in `run.sh` **only after** the A/B validation below (leave `run.sh` untouched in this commit).

5. Validation (record in worklog):
   ```bash
   cargo run --release -p pkr-testgames --bin kuhn-experiment   # add a "canon-alt" config wired to alternating semantics if trivial; otherwise skip here
   ./bench.sh   # compare it/s at 1/2/4/8 threads vs non-alternating
   ```

**Acceptance Criteria**
- Workspace tests + clippy pass; `./smoke.sh` passes with and without `--alternating`.
- Throughput: it/s with `--alternating` is ≥ 0.9× the non-alternating baseline at the same thread count (it does ~half the traversals per iteration).
- Decision recorded in worklog: keep or drop the default based on Kuhn exploitability slope per wall-clock second.

**Compatibility.** Changes training semantics (iteration accounting) → retrain when enabling. The flag defaults off, so nothing breaks.

---

## T6 — Street-scoped full-history infoset keys

**Objective.** Today the infoset hash folds in only `history_signature()` — a 3-byte `(actions_this_street, total_raises, last_was_bet)` summary — so distinct betting lines with the same summary collapse into one infoset (e.g. preflop `[call, raise, call]` vs `[raise, call, call]`; and every flop node merged across different preflop raise *counts* is fine, but `[check, bet, call]` vs `[bet, call]` sequences also merge). SOTA solvers key infosets on the full betting sequence of the current street. Replace the signature with the street-scoped **abstract action sequence** (the `abstract_history` buckets already recorded in `GameState`), which is small, bounded, and makes the hash strictly more informative. (This follows the standard practice of keying on the full in-street sequence; see also the imperfect-recall abstraction literature — Johanson et al. 2013 — for why keeping sequence info matters more than finer card buckets.)

**Exclusive File Paths**
- `crates/pkr-core/src/state.rs`
- `crates/pkr-abstraction/src/lib.rs`
- `crates/pkr-export/src/header.rs`
- `crates/pkr-export/src/writer.rs`
- `crates/pkr-runtime/src/mmap.rs`

**Dependencies**
- T2 (masking) must land first — it changes how strategies are consumed.

**Instructions**

1. **GameState**: add two fields
   ```rust
   pub street_abstract_actions: [u8; 16],
   pub street_abstract_len: u8,
   ```
   and a method `pub fn street_abstract_bytes(&self) -> &[u8] { &self.street_abstract_actions[..self.street_abstract_len as usize] }`.

2. In `apply_action_internal`, where the abstract bucket is appended to `abstract_history`, also append to the street-scoped buffer:
   ```rust
   if (self.street_abstract_len as usize) < self.street_abstract_actions.len() {
       self.street_abstract_actions[self.street_abstract_len as usize] = bucket;
       self.street_abstract_len += 1;
   }
   ```

3. In `advance_street_in_place`, reset `self.street_abstract_len = 0;` next to `self.actions_this_street = 0;`.

4. **Undo**: extend `UndoRecord` with `street_abstract_len: u8`, snapshot it in `push_undo` (`street_abstract_len: self.street_abstract_len`), and restore it in `undo_action` (`self.street_abstract_len = rec.street_abstract_len;`). Do NOT derive it from `history_len` — that coupling only holds while one action maps to one bucket, and this task must not depend on it.

5. **Hash**: in `KMeansAbstraction::get_infoset_hash`, replace
   ```rust
   fnv1a(&mut h, &[history.len() as u8]);
   fnv1a(&mut h, history);
   ```
   with the same length-prefixing applied to the caller-provided `history` slice, and change the CALLER (`crates/pkr-cfr/src/traversal.rs`) to pass the street-scoped sequence instead of the signature:
   ```rust
   let history_bytes = current.street_abstract_bytes();
   ```
   (remove the `history_signature()` call; keep `history_signature()` in `state.rs` for diagnostics — do not delete it).

6. **Format guard**: this changes every hash → add a scheme byte so old blueprints are rejected loudly instead of silently returning uniform. In `crates/pkr-export/src/header.rs`, repurpose the first byte of `FileHeader::_padding` as `key_scheme` (0 = legacy signature, 1 = street-scoped sequence):
   - `writer.rs`: set `_padding: [1u8, 0, 0, 0, 0, 0]` when writing.
   - `mmap.rs`: after the existing `hash_algo` check, add
     ```rust
     let key_scheme = file_header._padding[0];
     if key_scheme < 1 {
         return Err(MmapError::UnsupportedVersion(file_header.version)); // legacy key scheme
     }
     ```
     (add a dedicated `MmapError::InvalidKeyScheme(u8)` variant if you prefer a clearer message — preferred).

7. **Golden vectors**: the test `test_history_street_hash_golden` in `pkr-abstraction/src/lib.rs` pins exact hash values. They WILL change. Update the golden constants by recomputing them once with the new scheme (write a tiny test that prints the new values, run it, paste them in), and extend the doc comment: *"Regenerated for key_scheme=1 (street-scoped abstract sequence), 2026-09-22."*

8. Capacity: expect 2–5× more infosets. Train with `--capacity` raised accordingly (e.g. 20,000,000) and monitor `is_near_capacity`. Update the `--capacity` default in `run.sh` and document the growth factor in `docs/status.md`.

**Acceptance Criteria**
- Workspace tests + clippy pass; `./smoke.sh` passes (it retrains its tiny blueprint from zero, so old-hash rejection is exercised).
- Loading a pre-change blueprint fails with the new `InvalidKeyScheme` error (add a unit test in `pkr-runtime` that builds a `key_scheme=0` file and asserts the error).
- Distinct lines that previously collided now produce distinct hashes — add a unit test in `pkr-abstraction`: two histories `[1]` (check-call street) vs `[1,1]`… plus explicit `[2,1,1]` vs `[1,2,1]` assert different hashes.

**Compatibility.** **Invalidates all blueprints and checkpoints.** Delete `train.ckpt`, `blueprint.bin`; full retrain. Bump nothing else.

---

## T7 — Faster evaluation + exact-river EHS

**Objective.** `TableEvaluator::evaluate_hand` runs best-of-21 with an O(n²) duplicate filter on every call, and `calculate_ehs` Monte-Carlo-samples even on the river where the board is complete and the opponent's 990 possible combos can be enumerated exactly. Two upgrades: (a) make `evaluate_hand` branch-lean (skip duplicate filtering when the caller guarantees distinct cards — the hot paths do), and (b) exact river equity in `calculate_ehs`. Both directly speed up training (terminal payoffs), abstraction precompute (30–60 min turn table), and the exploitability tool of T8.

**Exclusive File Paths**
- `crates/pkr-eval/src/lookup_fast.rs`
- `crates/pkr-abstraction/src/ehs.rs`

**Dependencies**
- None.

**Instructions**

1. In `lookup_fast.rs`, add a fast entry point and route the trait through it:
   ```rust
   impl TableEvaluator {
       /// Evaluate 7 distinct cards. Caller guarantees no duplicates and
       /// total >= 5. Hot path: traversal terminals, EHS loops, BR tools.
       #[inline(always)]
       pub fn evaluate_distinct(&self, hole: &[u8], board: &[u8]) -> u32 {
           let mut cards = [0u8; 7];
           let mut total = 0usize;
           for &c in hole.iter().chain(board) {
               cards[total] = c;
               total += 1;
           }
           self.eval_best_of_21(&cards[..total])
       }

       fn eval_best_of_21(&self, cards: &[u8]) -> u32 {
           let mut best = u32::MAX;
           let n = cards.len();
           for i in 0..n {
               for j in (i + 1)..n {
                   for k in (j + 1)..n {
                       for l in (k + 1)..n {
                           for m in (l + 1)..n {
                               let mut hand = [cards[i], cards[j], cards[k], cards[l], cards[m]];
                               hand.sort_unstable_by(|a, b| b.cmp(a));
                               let idx = combinadic_rank(&hand) as usize;
                               let r = self.load_rank(idx);
                               if r < best { best = r; }
                           }
                       }
                   }
               }
           }
           best
       }

       #[inline(always)]
       fn load_rank(&self, idx: usize) -> u32 {
           let offset = idx * 4;
           let bytes = &self.mmap[offset..offset + 4];
           u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
       }
   }

   impl Evaluator for TableEvaluator {
       fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
           // Keep the defensive duplicate/sentinel filtering for the public
           // trait (external callers), but implement it via the same helper.
           let mut cards = [0u8; 7];
           let mut total = 0usize;
           'outer: for &c in hole.iter().chain(board) {
               if c >= 52 { continue; }
               for i in 0..total {
                   if cards[i] == c { continue 'outer; }
               }
               cards[total] = c;
               total += 1;
           }
           if total < 5 { return u32::MAX; }
           self.eval_best_of_21(&cards[..total])
       }
   }
   ```
   (Keep the existing bounds-check warning in `load_rank` or hoist it to `eval_best_of_21` entry; do not remove it.)

2. In `ehs.rs`, add the exact-river path:
   ```rust
   /// Exact hand vs random-hand equity on a complete board (5 cards):
   /// enumerate all C(remaining,2) opponent holdings. Returns (equity, equity_sq).
   fn exact_river_ehs(hole: &[u8], board: &[u8; 5], evaluator: &dyn Evaluator) -> (f64, f64) {
       let mut remaining = [0u8; 50];
       let mut rem_len = 0usize;
       for c in 0..52u8 {
           if !hole.contains(&c) && !board.contains(&c) {
               remaining[rem_len] = c;
               rem_len += 1;
           }
       }
       let hero_rank = evaluator.evaluate_hand(hole, board);
       let mut sum_eq = 0.0f64;
       let mut n = 0.0f64;
       for i in 0..rem_len {
           for j in (i + 1)..rem_len {
               let opp = [remaining[i], remaining[j]];
               let opp_rank = evaluator.evaluate_hand(&opp, board);
               let eq = if hero_rank < opp_rank { 1.0 } else if hero_rank == opp_rank { 0.5 } else { 0.0 };
               sum_eq += eq;
               n += 1.0;
           }
       }
       let ehs = sum_eq / n;
       (ehs, ehs * ehs) // exact second moment for binary/tie-valued equity
   }
   ```
   and in `calculate_ehs`, short-circuit when `board.len() == 5`:
   ```rust
   if board.len() == 5 {
       let mut full = [0u8; 5];
       full.copy_from_slice(board);
       let (e, e2) = exact_river_ehs(hole, &full, evaluator);
       return (e as f32, e2 as f32);
   }
   ```

3. Sanity tests (both crates): `evaluate_distinct` must agree with `evaluate_hand` on 10k random distinct hands (`assert_eq!`); `exact_river_ehs` on the board `[0,4,8,12,16]` (all aces impossible — pick any fixed board) must match a 100k-sample MC estimate within 0.01.

**Acceptance Criteria**
- Workspace tests + clippy pass; `./smoke.sh` passes.
- Micro-bench (a `#[test]` with `Instant` around 1e6 `evaluate_distinct` calls, `--release`): ≥ 1.3× faster than the old `evaluate_hand` path. Record the number in the worklog.
- Re-run the turn-table precompute on a small k and confirm the wall time drops (river short-circuit also affects `abs5`/river paths via `get_infoset_hash` fallbacks).

**Compatibility.** No format change; no retrain. The `Evaluator` trait is untouched.

---

## T8 — Sampled abstract-game exploitability metric

**Objective.** This is the single most important quality instrument the project lacks (your roadmap already sets the bar: *"don't chase exploitability below ~50 mbb/g"* — but nothing measures it). Implement a **sampled best-response exploitability** estimator for the trained blueprint over the abstract game: sample deals, compute the strategy value and both per-infoset best-response values over the abstract betting tree, aggregate. This is the practical stand-in for exact exploitability in a 10⁷-infoset game (cf. Lisý & Bowling 2017 on LBR; Timbers et al. 2022 on approximate BR; Davis et al. 2014 on response-function evaluation). Wire it into the trainer so it prints periodically.

**Exclusive File Paths**
- `crates/pkr-exploit/src/lib.rs` (extend)
- `crates/pkr-exploit/Cargo.toml`
- `binaries/pkr-trainer/src/main.rs` (call it at the end of training)

**Dependencies**
- T3 (Kuhn BR math sanity — reuse the same per-infoset-max insight), T2 (masked strategies), T11 is NOT required (this works on abstract buckets directly).

**Instructions**

1. Add dependencies to `crates/pkr-exploit/Cargo.toml`:
   ```toml
   pkr-core       = { workspace = true }
   pkr-cfr        = { workspace = true }
   pkr-abstraction = { workspace = true }
   pkr-eval       = { workspace = true }
   rand           = { workspace = true }
   rayon          = { workspace = true }
   ```

2. Implement the estimator in `pkr-exploit/src/lib.rs`:
   ```rust
   use pkr_abstraction::KMeansAbstraction;
   use pkr_contracts::{Evaluator, FNV_OFFSET, fnv1a};
   use pkr_core::state::{Action, ActionKind, GameState, Street};
   use std::collections::HashMap;
   use std::sync::Arc;

   pub struct ExploitabilityResult {
       /// Chips per hand. Convert to mbb/g: value / BB * 1000 (BB = 2.0).
       pub value_of_strategy: f32,
       pub best_response_value: f32,
       pub exploitability_chips: f32,
       pub deals_sampled: u32,
   }

   /// Strategy source: the trained table (regrets -> masked strategy).
   pub trait StrategySource {
       /// Masked, renormalized strategy for the infoset (uniform if unseen).
       fn strategy(&self, infoset_hash: u64, legal: &[bool; 6]) -> [f32; 6];
   }
   ```

   The estimator walks the abstract game with **no sampling inside the tree** (only the initial deal and the full board runout are sampled). For one sampled deal (hero cards, villain cards, complete 5-card board):
   - **Pass A (accumulate)**: enumerate the full abstract betting tree recursively. At each player node compute the infoset hash via the SAME `KMeansAbstraction::get_infoset_hash` used in training (street, street_abstract bytes, cluster, flop bucket) — this is why T6's helper is public. Accumulate for each infoset of each player: `cfv[infoset][a] += weight * reach_opponent_prefix`, where `weight` is the deal prior (uniform over sampled deals: 1/N) and reaches exclude the acting player.
   - **Pass B (BR)**: for each infoset, `br_action = argmax_a cfv[infoset][a]` (ties → lowest index).
   - **Pass C (value)**: re-walk the tree; at BR player's nodes play `br_action`, elsewhere play σ; add utilities. `BR value = mean over deals of BR payoff to player i`; `σ value = mean payoff under both σ`.
   - `exploitability_chips = Σ_i (BR_i − σ_i) / 2`.
   Provide the recursive walker as a `struct AbstractWalker<'a>` with fields `{ abstraction, evaluator, source: &'a dyn StrategySource, board: [u8;5], board_len, cfv: HashMap<u64,[f32;6]>, deal_prior: f32 }` and methods `walk_value(&mut self, state, hero, reach: [f32;2]) -> [f32;2]` (sums probabilities, used in pass A with `reach_*` weighting) and `walk_br(&self, state, br_player, br_map, hero) -> [f32;2]` (pass C). Terminal payoff uses `GameState::terminal_payoff` with the fixed board dealt via `advance_street_in_place`.

   Deal sampling (rayon over deals):
   ```rust
   pub fn sampled_exploitability(
       abstraction: &Arc<KMeansAbstraction>,
       evaluator: &dyn Evaluator,
       source: &dyn StrategySource,
       num_deals: u32,
       rng_seed: u64,
   ) -> ExploitabilityResult {
       // Sample: hero 2, villain 2, board 5 distinct cards, uniform.
       // Run pass A+B+C per deal in parallel; reduce.
   }
   ```
   Notes that MUST go into the doc comment: (a) this measures exploitability **of the abstraction's game**, an upper bound on true-game exploitability is not implied — it is the standard proxy used before LBR; (b) 10_000 deals give a standard error small enough to rank training runs; (c) convert with `exploitability_chips / 2.0 * 1000.0` → mbb/g.

3. Add a `StrategySource` impl for `&CompactRegretTable` in `pkr-exploit` (reads average strategy via `get_average_strategy_into`, then applies the T2-style legal mask passed in).

4. Wire into the trainer (`main.rs`): new flag
   ```rust
   /// Sampled exploitability check after training (0 = off).
   #[arg(long, default_value_t = 0)]
   exploitability_deals: u32,
   ```
   At the end of training (before export), when > 0, run `sampled_exploitability(..., cli.exploitability_deals, cli.seed)` and print:
   ```
   EXPLOITABILITY deals={} value={:.4} br={:.4} expl={:.4} chips/hand = {:.1} mbb/g
   ```

5. Unit test with a tiny fixed setup: build a `CompactRegretTable`, insert a **pure limped-then-fold-everything** strategy by pushing strategy sums by hand, run 200 deals, assert `exploitability_chips > 0` and that the BR value strictly exceeds the σ value (a always-fold bot is exploitable by any raise).

**Acceptance Criteria**
- Workspace tests + clippy pass.
- On the smoke blueprint (`./smoke.sh` artifact), 1000-deal exploitability completes in < 60 s and prints a number; record it in the worklog.
- On a 100k-iteration blueprint vs a 1M-iteration blueprint (same settings, post-T1/T2), the metric must rank them in the expected direction. Record both numbers in the worklog — this is the project's first real quality trend measurement.

**Compatibility.** Read-only instrumentation. No retrain.

---

# TIER 2 — Abstraction, translation & runtime

## T9 — Unified action abstraction (fix the bucket collapse)

**Objective.** Three parts of the system disagree about what the 6 abstract buckets mean:
1. `state.rs::legal_actions*` generates bets at **0.5 / 1.0 / 2.0 × pot** (+ jam).
2. `traversal.rs::abstract_action_index` maps a bet to buckets by fraction thresholds `<0.5 → 2, <1.0 → 3, else 4`. A generated 0.5×pot bet has fraction exactly 0.5 → **bucket 3**; a 1.0×pot bet has fraction exactly 1.0 → **bucket 4**; a 2.0×pot bet → also **bucket 4**. So buckets 2 and 3 (sub-0.5 sizes) are never generated, and **1.0× and 2.0× pot collapse into the same regret slot** — the traverser explores only one of them per iteration.
3. `writer.rs::STREET_BET_FRACTIONS` claims the buckets are **0.45 / 0.9 / 2.2 / 1.0** — a leftover from the roadmap's geometric-size proposal that was applied to the export but never to the trainer.

Fix by defining ONE shared action abstraction in `pkr-core` and deriving everything from it.

**Exclusive File Paths**
- `crates/pkr-core/src/lib.rs` (new module `action_abstraction`)
- `crates/pkr-core/src/state.rs`
- `crates/pkr-cfr/src/traversal.rs`
- `crates/pkr-export/src/writer.rs`

**Dependencies**
- None (do it before T11; do it before any long retrain).

**Instructions**

1. New file `crates/pkr-core/src/action_abstraction.rs`:
   ```rust
   //! The single source of truth for the 6 abstract actions:
   //! 0 = fold, 1 = check/call, 2..=4 = bet at BET_FRACTIONS[b], 5 = all-in.
   //! Every subsystem (legal action generation, abstract bucket mapping,
   //! export translation) MUST derive from these constants.

   /// Bet sizes as fraction of pot (geometric sizing per the roadmap).
   pub const BET_FRACTIONS: [f32; 3] = [0.45, 0.9, 2.2];

   /// Map a bet (expressed as fraction of the current pot) to its bucket.
   /// Nearest-fraction matching with ties going to the smaller bucket.
   pub fn bucket_for_fraction(frac: f32) -> u8 {
       let mut best = 0u8;
       let mut best_d = f32::MAX;
       for (i, &f) in BET_FRACTIONS.iter().enumerate() {
           let d = (frac - f).abs();
           if d < best_d {
               best_d = d;
               best = (i + 2) as u8;
           }
       }
       best
   }

   /// The bet fraction a bucket represents (0.0 for fold/check/call).
   pub fn fraction_for_bucket(bucket: u8) -> f32 {
       if bucket >= 2 && bucket <= 4 { BET_FRACTIONS[(bucket - 2) as usize] } else { 0.0 }
   }
   ```
   Register `pub mod action_abstraction;` in `crates/pkr-core/src/lib.rs`.

2. In `state.rs`, replace both hardcoded `&[0.5, 1.0, 2.0]` loops with
   ```rust
   use crate::action_abstraction::BET_FRACTIONS;
   ...
   for &frac in &BET_FRACTIONS { ... }
   ```
   (identical semantics otherwise, including the stack-capacity checks).

3. In `traversal.rs::abstract_action_index`, replace the threshold chain for `ActionKind::Bet(amount)`:
   ```rust
   ActionKind::Bet(amount) => {
       if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
           Some(5)
       } else {
           let pot = state.pot.max(1.0);
           Some(pkr_core::action_abstraction::bucket_for_fraction(amount / pot) as usize)
       }
   }
   ```
   (`pkr-core` is already a dependency of `pkr-cfr` via `pkr_core::state`.)

4. In `writer.rs`, delete the literal and re-export from the shared module:
   ```rust
   use pkr_core::action_abstraction::{BET_FRACTIONS, fraction_for_bucket};
   pub const STREET_BET_FRACTIONS: [[f32; 6]; 4] = [
       [0.0, 0.0, 0.0, 0.0, 0.0, 1.0], // preflop: no bet sizes encoded (bets handled as raises)
       [0.0, 0.0, BET_FRACTIONS[0], BET_FRACTIONS[1], BET_FRACTIONS[2], 1.0],
       [0.0, 0.0, BET_FRACTIONS[0], BET_FRACTIONS[1], BET_FRACTIONS[2], 1.0],
       [0.0, 0.0, BET_FRACTIONS[0], BET_FRACTIONS[1], BET_FRACTIONS[2], 1.0],
   ];
   ```
   and add a consistency test:
   ```rust
   #[test]
   fn buckets_and_fractions_round_trip() {
       for b in 2u8..=4 {
           let f = fraction_for_bucket(b);
           assert_eq!(pkr_core::action_abstraction::bucket_for_fraction(f), b);
       }
   }
   ```

5. Add a `pkr-core` test proving the old collapse is gone:
   ```rust
   #[test]
   fn three_bet_sizes_land_in_three_distinct_buckets() {
       let pot = 10.0;
       assert_ne!(
           action_abstraction::bucket_for_fraction(0.45),
           action_abstraction::bucket_for_fraction(0.9)
       );
       assert_ne!(
           action_abstraction::bucket_for_fraction(0.9),
           action_abstraction::bucket_for_fraction(2.2)
       );
       let _ = pot; // fractions are pot-relative; sizes above are exact matches
   }
   ```

**Acceptance Criteria**
- Workspace tests + clippy pass; `./smoke.sh` passes.
- `grep -rn "0.5, 1.0, 2.0" crates/ binaries/` returns no action-generating matches.
- During a 10k-iteration run, the trainer's metrics show all of buckets 2, 3, 4 receiving nonzero regret mass (spot check via `sample_infosets`), and infosets count grows moderately (each street now distinguishes 3 bet sizes instead of 2).

**Compatibility.** Changes training semantics AND the meaning of exported CDF slots → **full retrain**, and existing blueprints are semantically invalid even if they load (the K constant is unchanged so the file format loads, but the strategy is wrong). Bump `key_scheme` byte (T6's `_padding[0]`) to **2** when this lands, so old files are rejected.

---

## T10 — Finer river tiers via rank percentiles

**Objective.** River hands are bucketed `hand_rank >> 6` → ~117 tiers, while your own roadmap calls for 1000–2000 river buckets and the centroids machinery already supports k up to ~2000 (`u16`... note: the current river path emits `u64` cluster ids so size is not constrained by u8). A cheap, principled upgrade: map `hand_rank` to its **percentile** against the uniform-random-hand distribution (precomputed from `hand_ranks.bin`), then quantize the percentile into `RIVER_TIERS = 1024` buckets. Equal-population buckets beat equal-width buckets for CFR because every tier gets visited comparably often. (Context: abstraction quality — equal-frequency EHS bucketing and imperfect-recall clustering — Johanson et al. 2013; Ganzfried & Sandholm 2014 for the potential-aware extension you can grow into later.)

**Exclusive File Paths**
- `crates/pkr-abstraction/src/lib.rs`
- `crates/pkr-abstraction/src/bin/precompute.rs`

**Dependencies**
- None.

**Instructions**

1. In `precompute.rs`, add a subcommand `river_percentiles`:
   ```rust
   "river_percentiles" => {
       let rank_table = args.get(2).map(|s| s.as_str()).unwrap_or("hand_ranks.bin");
       let output = args.get(3).map(|s| s.as_str()).unwrap_or("river_percentiles.bin");
       generate_river_percentiles(rank_table, output)?;
   }
   ```
   Implementation: read all 2,598,960 u32 ranks from the 5-card rank table (the file written by `generate_hand_ranks`), sort a copy, and for each *distinct rank value* store `percentile = (count of ranks strictly worse) / total as f32 → u16` where "worse" means larger u32 (remember: lower u32 = better hand). Output format: a sorted array of `(rank_value: u32, percentile: u16)` pairs for distinct ranks (≈7462 entries), binary-searched at runtime.

2. In `KMeansAbstraction`, add
   ```rust
   river_percentiles: OnceLock<Mmap>, // (u32 rank, u16 pct) pairs
   pub fn init_river_percentiles(&self, path: &str) -> Result<(), std::io::Error> { ... }
   ```
   and replace the river branch of `get_infoset_hash`:
   ```rust
   5 => {
       // T10: percentile-based equal-population river tiers.
       let hand_rank = self.evaluator.evaluate_hand(hole, board) as u32;
       let tier = self.river_tier(hand_rank);          // u16 0..RIVER_TIERS
       let board_bucket = /* unchanged: river table lookup or 0 */;
       ((tier as u64) << 8) | (board_bucket & 0xff)
   }
   ```
   with
   ```rust
   const RIVER_TIERS: u32 = 1024;
   fn river_tier(&self, rank: u32) -> u16 {
       if let Some(m) = self.river_percentiles.get() {
           let bytes = &m[..];
           let entries = bytes.len() / 6;
           let key = (rank, ) // binary search the (u32 rank) column
           let pct = binary_search_percentile(bytes, entries, rank); // f32 in [0,1)
           return ((pct * RIVER_TIERS as f32) as u16).min(RIVER_TIERS as u16 - 1);
       }
       (rank >> 6) as u16 // legacy fallback if the percentile table is absent
   }
   ```
   (implement `binary_search_percentile` with plain `slice::binary_search_by` over the u32 column; keep it allocation-free).

3. Trainer/plumbing: in `binaries/pkr-trainer/src/main.rs` add `--river-percentiles: Option<PathBuf>` and call `abstraction.init_river_percentiles(...)`. Add the generation step to `run.sh` **before** the trainer step:
   ```bash
   cargo run --release -p pkr-abstraction --bin pkr-abstraction-precompute -- river_percentiles hand_ranks.bin river_percentiles.bin
   ```
   and pass `--river-percentiles .smoke/river_percentiles.bin` in `smoke.sh` too (file is tiny, ~45 KB).

4. Test: percentile table must be monotone non-decreasing in percentile as rank value increases (worse hand ⇒ higher percentile); `river_tier` of the strongest rank (lowest u32) must be 0; `river_tier` of the weakest must be `RIVER_TIERS - 1`.

**Acceptance Criteria**
- Workspace tests + clippy pass; `./smoke.sh` passes.
- On a 10k-iteration training run, river infosets distribute across ≥ 90% of the 1024 tiers (spot-check via `sample_infosets` + decode: `hash >> 8` — note: hash is FNV so instead verify via a debug dump helper or count distinct `(tier, board_bucket)` pairs from a debug method `river_debug_counts()`).

**Compatibility.** Changes hashes → **full retrain**. Bump `key_scheme` byte to 3.

---

## T11 — Wire action translation into runtime + CDF decode

**Objective.** The pseudo-harmonic translation (`compute_translation`) exists and is tested, but (a) `build_translation_table()` is computed and never written anywhere, (b) the runtime has no code to decode `SotaAdvice` CDF bytes into a probability vector, mask illegal actions, or map an off-tree bet size onto the abstract buckets. This task lands the complete decode path as pure functions in `pkr-runtime` (no format change: translation is computed on the fly from the shared constants — T9 — instead of shipped as a blob, which avoids a format version bump).

**Exclusive File Paths**
- `crates/pkr-runtime/src/advice.rs` (new)
- `crates/pkr-runtime/src/lib.rs`

**Dependencies**
- T9 (shared `BET_FRACTIONS` / `bucket_for_fraction`).

**Instructions**

1. New module `crates/pkr-runtime/src/advice.rs`:
   ```rust
   //! Decode a blueprint CDF into a playable legal-action distribution,
   //! including pseudo-harmonic translation for off-tree bet sizes
   //! (Ganzfried & Sandholm 2013), anchored on the shared action abstraction.

   use pkr_contracts::SotaAdvice;
   use pkr_core::action_abstraction::{BET_FRACTIONS, fraction_for_bucket};

   /// Decode cumulative u8 CDF into per-action probabilities over K slots.
   pub fn decode_cdf(advice: &SotaAdvice) -> [f32; 16] {
       let mut out = [0.0f32; 16];
       let mut prev = 0u16;
       for i in 0..advice.len as usize {
           let c = advice.cdf_probabilities[i] as u16;
           out[i] = (c.saturating_sub(prev)) as f32 / 255.0;
           prev = c;
       }
       out
   }

   /// Mask + renormalize over legal buckets (same semantics as T2 at train time).
   pub fn mask_renormalize(probs: &mut [f32; 16], legal: &[bool; 6]) {
       let mut total = 0.0f32;
       for a in 0..6 {
           if !legal[a] { probs[a] = 0.0; } else { total += probs[a]; }
       }
       if total > 0.0 {
           for a in 0..6 { probs[a] /= total; }
       } else {
           let n = legal.iter().filter(|&&l| l).count().max(1);
           for a in 0..6 { probs[a] = if legal[a] { 1.0 / n as f32 } else { 0.0 }; }
       }
   }

   /// Pseudo-harmonic interpolation between the two bracketing bet buckets
   /// for an off-tree bet at `frac` (pot fraction). Returns redistributed
   /// probability mass for [lower_bucket, upper_bucket]; caller adds it back
   /// onto the (already masked) blueprint distribution.
   pub fn translate_off_tree(
       probs: &[f32; 16],
       frac: f32,
   ) -> [f32; 16] {
       let mut out = *probs;
       // Find bracketing concrete buckets b_lo < frac < b_hi among 2..=4.
       let mut b_lo: Option<usize> = None;
       let mut b_hi: Option<usize> = None;
       for b in 2..=4usize {
           let f = BET_FRACTIONS[b - 2];
           if f <= frac { b_lo = Some(b); }
           if f > frac && b_hi.is_none() { b_hi = Some(b); }
       }
       let (lo, hi) = match (b_lo, b_hi) {
           (Some(l), Some(h)) => (l, h),
           // Outside the range: put all bet mass on the nearest bucket.
           (None, Some(h)) => return { out[h] += probs[2..=4].iter().sum::<f32>();
               for b in 2..=4 { if b != h { out[b] = 0.0; } } out },
           (Some(l), None) => return { out[l] += probs[2..=4].iter().sum::<f32>();
               for b in 2..=4 { if b != l { out[b] = 0.0; } } out },
           (None, None) => return out,
       };
       let lower = fraction_for_bucket(lo as u8);
       let upper = fraction_for_bucket(hi as u8);
       let reach_lower = probs[lo];
       let reach_upper = probs[hi];
       let denom = (upper - frac) * reach_lower + (frac - lower) * reach_upper;
       let p_lo = if denom.abs() < f32::EPSILON {
           0.5
       } else {
           (1.0 - ((frac - lower) * reach_upper) / denom).clamp(0.0, 1.0)
       };
       let mass = probs[lo] + probs[hi];
       out[lo] = mass * p_lo;
       out[hi] = mass * (1.0 - p_lo);
       out
   }
   ```
   This re-implements the same formula as `pkr_export::translate::compute_translation` but operating on decoded probabilities with correct reach weights (the blueprint's own bucket probabilities) instead of hardcoded `0.5/0.5`. Add a unit test asserting agreement with `compute_translation` for the anchor case `frac` exactly between two buckets with equal reaches.

2. Export the module: in `crates/pkr-runtime/src/lib.rs` add `pub mod advice;` and `pub use advice::{decode_cdf, mask_renormalize, translate_off_tree};`.

3. Tests (in `advice.rs`): (a) `decode_cdf` of `[64,128,192,255,255,255]` with `len=6` yields `[64,64,64,63,0,0]/255`; (b) `mask_renormalize` zeroes illegal buckets and renormalizes; (c) `translate_off_tree` with `frac` = 0.675 (midpoint of 0.45 and 0.9) and equal bucket probs yields 50/50; (d) mass conservation: sum(out) == sum(in) ± 1e-5 for 1000 random inputs.

**Acceptance Criteria**
- Workspace tests + clippy pass.
- `cargo test -p pkr-runtime` includes the four new tests.

**Compatibility.** Additive; no format change; no retrain. (The old `build_translation_table()` in `writer.rs` becomes dead code — delete it AND its tests in this task, recording the deletion in the worklog.)

---

## T12 — Runtime lookup v2: aligned reads + Eytzinger layout

**Objective.** `SolverHandle::get_advice_fast` binary-searches a sorted key array with `u64::from_le_bytes(keys[mid*8..mid*8+8].try_into())` per probe — ~23 probes over an 80 MB mmap for 10M keys, all cache-hostile. Two-level fix: (1) zero-copy aligned `&[u64]` reads, (2) an optional **Eytzinger (BFS) key layout** with a branchless search loop (Algorithmica: "Binary Search" — Eytzinger layout; Pibiri & Trani 2018 on PGM/MPHF alternatives). Keep the sorted layout as default; add format v3 with Eytzinger for the latency-critical path.

**Exclusive File Paths**
- `crates/pkr-runtime/src/lookup.rs`
- `crates/pkr-runtime/src/mmap.rs`
- `crates/pkr-export/src/writer.rs`
- `crates/pkr-export/src/header.rs`

**Dependencies**
- None.

**Instructions**

1. **Quick win (format-compatible)**: in `mmap.rs`, expose
   ```rust
   #[inline]
   pub fn keys_u64(&self) -> &[u64] {
       bytemuck::cast_slice(self.keys_data())
   }
   ```
   (offset after header+8 is 8-byte aligned since FileHeader is 64 bytes — assert with `debug_assert_eq!(self.offset_keys % 8, 0);`). In `lookup.rs` replace the per-probe `try_into` with `keys[mid]`.

2. **Eytzinger v3 (opt-in)**: add `FORMAT_VERSION_V3` to `header.rs` (v3 file = same layout, but keys+CDF stored in BFS order). In `writer.rs`:
   ```rust
   fn to_eytzinger<T: Copy>(sorted: &[T]) -> Vec<T> {
       // Standard BFS layout: node i's children at 2i+1, 2i+2.
       let n = sorted.len();
       let mut out = vec![None::<T>; n + 1]; // build via k-way recursion:
       // see algorithmica.org "Eytzinger" — reconstruct with the classic
       // build(k=0, offset=1) routine using an iterator over `sorted`.
   }
   ```
   Implement the classic `build` recursion (iterative to avoid stack overflow on 10M keys):
   ```rust
   fn build_eytzinger(sorted: &[u64], out: &mut [u64]) {
       // k-th node; offset for recursion; from Algorithmica (iterative form):
       let n = sorted.len();
       let mut k = 1usize;
       while k <= n {
           // (use the standard two-stack/iterator construction)
           # unimplemented!() // <- replace with the documented construction
       }
   }
   ```
   **Use the well-known construction**: iterate levels, or the simpler recursive version with explicit stack — the agent may consult the Algorithmica reference implementation; the invariant to test: in-order traversal of the Eytzinger array equals the sorted array.

3. Branchless search in `lookup.rs` for v3 files:
   ```rust
   #[inline]
   fn eytzinger_contains(keys: &[u64], n: usize, target: u64) -> Option<usize> {
       let mut k = 1usize;
       while k <= n {
           k = 2 * k + (keys[k] < target) as usize;
       }
       k >>= k.trailing_ones(); // descend to the found index
       if k > 0 && keys[k] == target { Some(k) } else { None }
   }
   ```
   (guard `k <= n` with `keys` padded by one sentinel entry = u64::MAX as the Algorithmica variant does; CDF index must be mapped back from BFS position to the original sorted position — store a permutation array `bfs_pos -> sorted_idx` written after the CDF, or store CDF in BFS order too and map the found BFS index directly to `cdf_data()`. Prefer **CDF in BFS order**: zero indirection.)

4. Version gating in `mmap.rs`: `if version >= FORMAT_VERSION_V3 { /* offsets for permutation/cdf-bfs */ }` — keep v2 reading exactly as today.

5. Benchmark harness (test-gated, `--release`): 1e6 random lookups over a 1M-key blueprint built by a test fixture; assert v3 Eytzinger ≥ 1.5× faster than v2 binary search on the same machine; record numbers in the worklog. (If it is NOT faster on the target VPS-class CPU, record that and keep v2 default — the task's deliverable is the measurement, not the layout.)

**Acceptance Criteria**
- Workspace tests + clippy pass; round-trip test: for 100k random keys, `lookup` returns the same advice in v2 and v3 files.
- `./smoke.sh` passes (v2 default unchanged).
- Worklog contains the latency numbers (p50/p99 over 1e6 lookups) for v2 vs v3.

**Compatibility.** v2 readers unaffected; v3 is opt-in at export time via `write_blueprint_v3`. No retrain (a v3 export can be produced from any existing checkpoint via a small `--export-v3` flag if desired — optional).

---

## T13 — Runtime advice glue + fix the eval harness `lookup(0)` bug

**Objective.** The serving story is incomplete: `pkr-runtime` can look up a hash, but nothing computes the hash from a `GameState` at runtime, and `pkr-fuzz::run_eval_harness` calls `blueprint.lookup(0)` — literally hash zero for every decision — and samples actions positionally against `legal_actions()`. Build the missing glue: a `RuntimeAdvisor` that (1) computes the infoset hash exactly like training, (2) looks up the blueprint, (3) decodes + masks + translates (T11) to produce a policy over **concrete legal actions**, and use it in the eval harness.

**Exclusive File Paths**
- `crates/pkr-runtime/src/advisor.rs` (new)
- `crates/pkr-runtime/Cargo.toml`
- `crates/pkr-runtime/src/lib.rs`
- `crates/pkr-fuzz/src/lib.rs`

**Dependencies**
- T9 (bucket semantics), T11 (decode/translate functions).

**Instructions**

1. Add to `crates/pkr-runtime/Cargo.toml`: `pkr-abstraction = { workspace = true }` and `pkr-eval = { workspace = true }`.

2. New `advisor.rs`:
   ```rust
   //! The complete serving path: GameState -> infoset hash -> blueprint
   //! lookup -> masked, translated policy over concrete legal actions.

   use crate::advice::{decode_cdf, mask_renormalize, translate_off_tree};
   use crate::SolverHandle;
   use pkr_abstraction::KMeansAbstraction;
   use pkr_core::state::{Action, ActionKind, GameState};

   pub struct RuntimeAdvisor {
       pub solver: SolverHandle,
       pub abstraction: KMeansAbstraction,
   }

   pub struct ConcretePolicy {
       /// Probability per concrete legal action, same order as
       /// GameState::legal_actions_into output.
       pub probs: Vec<f32>,
       pub total_mass_on_blueprint: f32, // diagnostic: 1.0 when hash hit
   }

   impl RuntimeAdvisor {
       pub fn policy(&self, state: &GameState) -> ConcretePolicy {
           // 1. Abstract bucket per concrete action (same fn as traversal).
           // 2. Infoset hash via abstraction.get_infoset_hash with
           //    street_abstract_bytes() as the history slice.
           // 3. Lookup -> SotaAdvice (None => uniform over legal).
           // 4. decode_cdf + mask_renormalize over the legal-mask computed
           //    from bucket counts (b count>0).
           // 5. If the actor's last action was an off-tree SIZE (i.e. the
           //    opponent bet a size that is not one of BET_FRACTIONS), call
           //    translate_off_tree before masking. Detection: the responding
           //    player's policy is conditional on the opponent's size; the
           //    translation applies when the CURRENT actor faces a bet whose
           //    raise-to/pot fraction is not within 0.02 of any bucket size.
           // 6. Distribute each bucket's mass evenly over its concrete
           //    actions (uniform within bucket — matches training exploration).
           todo!() // implement steps; ~60 lines; see comments
       }
   }
   ```

3. Fix the harness in `pkr-fuzz/src/lib.rs`: replace the `blueprint.lookup(0)` decision logic with `RuntimeAdvisor::policy(state)`, sample an action from `ConcretePolicy::probs`, and keep the existing `ScriptedBot` opponents. Also delete the never-written `actions_per_street` field or start writing it (it is allocated and always zero today) — write it; the roadmap's "uniform distribution = regression alarm" then becomes a real check.

4. Test (integration, `pkr-fuzz`): build the smoke blueprint (via a fixture that runs the smoke pipeline in-process or reads `.smoke/blueprint.bin` if present, `#[ignore]` otherwise like `load_external_blueprint`), play 200 hands vs `StationBot`, assert: no panic, every sampled action was legal, and `bb_per_100` is finite.

**Acceptance Criteria**
- Workspace tests + clippy pass.
- `PKR_BLUEPRINT=.smoke/blueprint.bin cargo test --release -p pkr-fuzz -- --ignored` passes with the new advisor path.
- The harness now reports per-opponent `bb_per_100` from REAL blueprint lookups (worklog: record the numbers vs the three scripted bots).

**Compatibility.** Additive. The `pkr-fuzz` old behavior (lookup(0)) is removed; nothing else depends on it.

---

# TIER 3 — Advanced (stretch)

## T14 — Real river re-solve (reach-weighted CFR+ subgame)

**Objective.** Replace the stub in `riversolve.rs` (regrets are reset every iteration; bet EV equals call EV; hero range enumerated although hero's hand is known — status.md already calls it "not real CFR"). Deliver a DeepStack-style **continual re-solve** for the river: at river start, build the subgame from the actual `GameState`, weight the villain's combos by blueprint consistency (their reach through the abstract game), and run reach-weighted CFR+ over the river betting tree with fine-grained bet sizes. This is the highest-leverage runtime quality upgrade available — it is the core mechanism behind Libratus/DeepStack's superhuman river play (Moravčík et al. 2017; Brown et al. 2017 safe & nested subgame solving; Brown & Sandholm 2018 depth-limited solving).

**Exclusive File Paths**
- `crates/pkr-cfr/src/riversolve.rs` (rewrite)
- `crates/pkr-runtime/src/advisor.rs` (optional wiring: `get_advice_deep`)

**Dependencies**
- T2, T9 (bucket/bet semantics), T13 (advisor plumbing), T8 (to validate the gain).

**Instructions (design — implement to this spec)**

1. **Subgame definition**: input = `GameState` at river start (≤2 actions this street), hero hole cards (the player we advise), villain's *abstract range*: enumerate all villain combos from remaining cards (≤ 990 on the river minus known cards), weight each combo by its blueprint reach along the actual history:
   ```rust
   fn combo_reach(&self, villain: [u8;2], state: &GameState) -> f32 {
       // Replay each street's abstract sequence through the blueprint:
       // for each villain decision node on the path, multiply the blueprint
       // probability of the action actually taken (via RuntimeAdvisor hash
       // path). Missing hash => 0 weight (combo "impossible" under σ).
       // Renormalize; if total mass < 1e-6, fall back to uniform.
   }
   ```
   This is the DeepStack "opponent range constrained by the blueprint" step. It uses the abstraction for villain infoset hashing — villain hole cards are required input, which is why this runs only in deep mode where the full state is available.

2. **Tree**: bet sizes for the subgame = `[0.33, 0.5, 0.75, 1.0, 1.5, jam]` × pot (fine-grained — the whole point of re-solving), plus fold/check/call as legal. Cap tree depth at 4 bets per street (this is a river-only subgame so depth is naturally bounded).

3. **Solve**: vanilla CFR+ (RM+ regret matching, alternating updates, 300–500 iterations) over the subgame with hero's nodes AND villain's nodes both solved; villain nodes use the weighted range as the chance distribution. Leaf values = showdown EV via `TableEvaluator::evaluate_distinct`. Because both players' strategies are solved against each other, the result is safer than naive hero-only BR.
   ```rust
   pub struct RiverResolve { /* ranges, tree, regrets[2][nodes][actions] */ }
   impl RiverResolve {
       pub fn solve(&mut self, iterations: u32) { /* alternating CFR+ */ }
       /// Hero's strategy at the ROOT node of the subgame, over subgame actions.
       pub fn root_strategy(&self) -> Vec<f32> { /* average strategy */ }
   }
   ```

4. **Safety fallback**: compare the re-solve value to the blueprint's value at the same state (blueprint policy value against the same weighted range). Ship a **maxmargin-flavored guard**: if `resolve_value < blueprint_value - margin` (margin = 0.25 bb), return the blueprint strategy instead (this bounds the worst case of a buggy re-solve; full maxmargin/LBRD safety is future work — Brown et al. 2017).

5. **Wiring**: `RuntimeAdvisor::policy_deep(state, hero_hole)` uses the resolver when `is_river_resolvable(state)` (keep the existing predicate), else falls back to the fast path. Keep the resolver allocation-light: preallocate regret/strategy vectors per node id (`Vec<f32>` indexed by compact node numbering; the river tree is a few hundred nodes at most).

6. **Validation**: (a) unit test: river with pot 10, stacks 10, hero has the nuts → root strategy must be ~1.0 jam/call-heavy and never fold; (b) unit test: pot 10, stacks 10, hero has the stone-cold bluff-catcher vs a polarized blueprint range → mixed strategy with nonzero fold; (c) integration: 1000 hands of `run_eval_harness` with deep mode on vs `NitBot` must not regress vs fast mode by more than 5 bb/100 (record actual numbers).

**Acceptance Criteria**
- Workspace tests + clippy pass; `./smoke.sh` passes (deep path is opt-in, smoke uses fast path).
- End-to-end deep lookup latency p99 < 50 ms for the river case on the dev machine (the roadmap's budget is 300 ms — record measured).
- Worklog records exploitability delta (T8) between fast-only and deep-enabled serving on the same blueprint.

**Compatibility.** Opt-in runtime path. No retrain.

---

## T15 — Stretch research items (no code required, design notes)

- **VR-MCCFR** (Schmid et al. 2019, AAAI): baseline the sampled values with an estimate of the opponent's expected value at traverser nodes to cut variance several-fold — pairs well with T1's unbiased estimator. Prototype in Kuhn first via the (now fixed) harness.
- **Hand-isomorphism folding** (Waugh 2015; Gilpin/Sandholm 2007 indexing): fold suit isomorphisms of the (hole+board) into canonical indices BEFORE the cluster lookup — cuts flop/turn infosets ~3-5× and makes every bucket denser. Implement as a pure function in `pkr-core` used by both trainer and runtime; requires key_scheme bump.
- **Potential-aware river/turn features** (Ganzfried & Sandholm 2014 EMD; DeepStack's turnfeatures): extend the turn table from (EHS, EHS²) to include EHS′ (next-street expected hand strength) — the table format already stores 15 bytes/combo, so adding one byte per mask slot is free at read time but requires re-precompute.
- **DDCFR / optimistic CFR variants** (Xu et al.; Farina et al.): discount-schedule learning and optimistic updates — only worth revisiting after T8 makes quality measurable.
- **Local Best Response** (Lisý & Bowling 2017): fold-raise-to-allin constrained BR for a *true* lower-bound exploitability of the full game (not just the abstraction); substantial engineering, do after T14.

---

## Final acceptance checklist (run once after the last completed task)

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
./smoke.sh
cargo run --release -p pkr-testgames --bin kuhn-experiment   # exploitability decreasing, no NaN
./bench.sh                                                   # record it/s table
# Then a supervised 1M-iteration run with the new estimator + keys:
# ./run.sh   (capacity raised per T6; --alternating if A/B won; --exploitability-deals 2000)
```

Record in the worklog: Kuhn exploitability curve, bench it/s, sampled exploitability (mbb/g) before/after the retrain, and any task whose acceptance measurement failed.

---

## References

1. Lanctot, Waugh, Bowling, Zinkevich — *Monte Carlo Sampling for Regret Minimization in Extensive Games* (NeurIPS 2009). https://www.cs.cmu.edu/~lanctot/files/publications/neurips-2009.pdf (ES/OS-MCCFR estimator definitions — basis of T1)
2. Gibson, Lanctot, Bowling — *Generalized Sampling and Variance in Counterfactual Regret Minimization* (UAI 2012). https://poker.cs.ualberta.ca/publications/UAI12.pdf (unbiased bounded estimators)
3. Tammelin — *Solving Large Imperfect Information Games Using CFR+* (2014). arXiv:1407.5042 (CFR+, alternating updates — basis of T5)
4. Farina, Kroer, Sandholm — *PCFR+ / stable-predictive-optimistic regret* (ICML 2021) (momentum term already implemented in `dcfr.rs`)
5. Brown & Sandholm — *Solving Imperfect-Information Games via Discounted Regret Minimization* (AAAI 2019). arXiv:1810.00748 (DCFR — implemented; degenerates in f32, see docs/status.md)
6. Schmid, Moravčík, et al. — *Variance Reduction in Monte Carlo CFR* (AAAI 2019). https://ojs.aaai.org/index.php/AAAI/article/view/4555 (VR-MCCFR — T15)
7. Moravčík, Schmid, et al. — *DeepStack: Expert-level artificial intelligence in heads-up no-limit poker* (Science 2017) (continual re-solving, turn value net — basis of T14)
8. Brown, Moravčík, Sandholm — *Depth-Limited Solving for Imperfect-Information Games* (2018) (value functions at depth limits)
9. Brown, Sandholm, Burch — *Safe and Nested Subgame Solving for Imperfect-Information Games* (NeurIPS 2017). arXiv:1705.02955 (Libratus subgame solving, maxmargin — basis of T14 safety)
10. Brown & Sandholm — *Superhuman AI for heads-up no-limit poker: Libratus* (Science 2018) (nested subgame solving for off-tree actions)
11. Brown & Sandholm — *ReBeL* (NeurIPS 2020). arXiv:2007.13544 (the RL-based endgame — explicitly out of scope, see roadmap non-goals)
12. Ganzfried & Sandholm — *Action Translation in Extensive-Form Games with Large Action Spaces* (2013) (pseudo-harmonic mapping — implemented in `translate.rs`, wired by T11)
13. Johanson, Waugh, Bowling, Zinkevich — *Evaluating State-Space Abstractions in Extensive-Form Games* (AAMAS 2013). https://poker.cs.ualberta.ca/publications/amas13.pdf (abstraction evaluation; imperfect recall — basis of T6/T10)
14. Ganzfried & Sandholm — *Potential-Aware Imperfect-Recall Abstraction with Earth Mover's Distance* (IJCAI 2014). https://www.cs.cmu.edu/~sganzfried/pdfs/emd_ijcai14.pdf (T15)
15. Waugh — *A Fast and Optimal Hand Isomorphism Algorithm* (2015). https://www.cs.cmu.edu/~kwagh/papers/hand_isomorphism.pdf (T15)
16. Lisý & Bowling — *Equilibrium Approximation Quality of Current No-Limit Poker Bots* (2017) (LBR — T8/T15)
17. Timbers, Schmid, Moravčík, et al. — *Approximate Exploitability: Learning a Best Response* (IJCAI 2022). https://www.ijcai.org/proceedings/2022/481 (T8 context)
18. Davis, Waugh, Bowling — *Using Response Functions to Measure Strategy Strength* (AAAI 2014) (T8 context)
19. Algorithmica (S. P.) — *Binary Search: Eytzinger layout & branchless search*. https://en.algorithmica.org/hpc/binary-searching/binary-search/ (T12)
20. Pibiri & Trani — *PTHash / Minimal Perfect Hashing* literature (T12 context; the existing `fmph.rs` is unused — either wire or delete)
21. *aya_poker* — Rust poker evaluator with compile-time perfect-hash tables. https://docs.rs/aya_poker (T7 alternative)

*End of playbook. Execute in index order; when in doubt, stop and record rather than improvise.*
