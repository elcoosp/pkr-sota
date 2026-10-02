# pkr-sota — Full Code Review: Bugs & Code Smells

**Repo:** `github.com/elcoosp/pkr-sota` @ `cce1339` (main)
**Scope:** 105 Rust files / ~31,700 LOC across 11 crates + `pkr-trainer` binary, 8 shell scripts, CI workflows, and docs.
**Method:** Manual line-by-line review of every non-test source file, cross-checked against the project's own audit trail (`docs/handoffs/*`, `docs/experiments/training-nondeterminism.md`, the F1–F9 audit table in `docs/status.md`, and the B1–B23 worklog). Every finding below was verified against the actual code; previously-fixed items are *not* re-reported as live bugs (see §6 for their verification status).

> **Note:** this report was produced by static review. The review environment has no Rust toolchain, so run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo nextest run` after applying fixes. Each fix below is written to compile against the current code.

---

## Executive summary

| Severity | Count | Meaning |
|---|---|---|
| **High** | 3 | Wrong results / corrupt artifacts / UB in shipped or shipped-adjacent paths |
| **Medium** | 6 | Broken features, silent no-ops, or pathological behavior on reachable configs |
| **Low** | 7 | Latent bugs behind flags, weak input validation, minor correctness edges |
| **Code smells** | 15 | Dead/misleading APIs, unsafe patterns, perf waste, library hygiene |
| **Doc bugs** | 6 | Stale or self-contradictory comments that already caused (or will cause) incidents |

The codebase is in unusually good shape for its size — it has survived three prior audit rounds, and the CFR core (`traversal.rs`, `table.rs`, `dcfr.rs`), the game state machine (`state.rs`), the evaluators, and the export/runtime binary formats are correct as far as this review could establish. The remaining problems cluster in three areas:

1. **The two "solver" stubs** (`pkr-cfr/src/riversolve.rs`, `pkr-exploit/src/public_br.rs`) — one is mathematically broken with a doc that overclaims, the other is honestly labeled WIP but still `pub`.
2. **Blend/aggregation edge cases** in the subgame solver (`blend_p0_strategy`) and the trainer's `--min-visits` filter — both produce silently wrong output rather than crashing.
3. **Concurrency leftovers**: the documented-but-unfixed `alloc_idx` orphan race (which this review links to the still-unexplained 1e-11 checkpoint checksum drift), and a genuine `&mut` aliasing pattern in `pkr-subgame`'s parallel solve.

---

## 1. High severity

### H1. `blend_p0_strategy` produces non-normalized (or α-invariant) strategies when one side is missing

**File:** `crates/pkr-subgame/src/lib.rs:107–141`

`safe_solve` blends the CFR strategy with the blueprint strategy to guarantee "the shipped strategy is never more exploitable than the blueprint." Its doc says: *"Missing entries in either default to uniform."* The code does not implement that:

```rust
(Some(sa), None) => {
    let mut s = [0.0; ABSTRACT_BUCKETS];
    for k in 0..ABSTRACT_BUCKETS {
        s[k] = alpha * sa[k] + (1.0 - alpha) * sa[k];   // == sa[k] for every alpha!
    }
    ...
}
(None, Some(sb)) => {
    let mut s = [0.0; ABSTRACT_BUCKETS];
    for k in 0..ABSTRACT_BUCKETS {
        s[k] = (1.0 - alpha) * sb[k];                   // sums to (1-alpha), not 1
    }
    ...
}
```

Two distinct defects:

* **`(Some, None)` branch:** `alpha*sa + (1-alpha)*sa` is an algebraic no-op. The blueprint-missing side never receives the promised uniform fallback, so the binary search over `alpha` sees a constant value on those nodes and can return a wrong `best_alpha`.
* **`(None, Some)` branch:** probabilities sum to `1-alpha`. At `alpha = 1.0` the node plays an **all-zero strategy**; in `br_walk` a P0 node with a zero strategy contributes `0.0` to every branch, which corrupts the very safety metric `safe_solve` is trying to bound.

Both cases are reachable: `build_blueprint_strategy` emits `None` when the blueprint's cluster puts all mass on buckets that are illegal at the concrete node (`sum ≤ 1e-12`, `lib.rs:1208`), and `p0_strategy` emits `None` when a deal's strategy-sum underflowed to zero (`lib.rs:843`).

**Fix** (uniform over the present side's support — both sides share the same legal-bucket mask):

```rust
(Some(sa), None) => {
    let n = sa.iter().filter(|&&p| p > 0.0).count().max(1) as f64;
    let u = 1.0 / n;
    let mut s = [0.0; ABSTRACT_BUCKETS];
    for k in 0..ABSTRACT_BUCKETS {
        let fallback = if sa[k] > 0.0 { u } else { 0.0 };
        s[k] = alpha * sa[k] + (1.0 - alpha) * fallback;
    }
    out.push(Some(s));
}
(None, Some(sb)) => {
    let n = sb.iter().filter(|&&p| p > 0.0).count().max(1) as f64;
    let u = 1.0 / n;
    let mut s = [0.0; ABSTRACT_BUCKETS];
    for k in 0..ABSTRACT_BUCKETS {
        let fallback = if sb[k] > 0.0 { u } else { 0.0 };
        s[k] = alpha * fallback + (1.0 - alpha) * sb[k];
    }
    out.push(Some(s));
}
```

Add a regression test asserting: (a) `blend(Some, None, 0.5)` differs from `sa` and sums to 1; (b) `blend(None, Some, 1.0)` is uniform, not zeros.

---

### H2. `--min-visits` compares a *normalized* strategy sum (always ≈ 1.0) against a visit threshold

**File:** `binaries/pkr-trainer/src/main.rs:256–262`

```rust
if min_visits > 0.0 {
    keys.retain(|k| {
        table
            .get_average_strategy_slice(*k)
            .is_some_and(|s| s.iter().sum::<f32>() >= min_visits)
    });
}
```

`get_average_strategy_slice` returns `compute_export_strategy` — the **normalized** average strategy (sums to 1.0), with a regret-matched/uniform fallback that *also* sums to 1.0 (the B10 fix). Therefore:

* `--min-visits 0.5` (or any value ≤ 1.0) filters **nothing** — the documented "skip infosets whose reach-weighted strategy mass is below this many visits" never happens.
* `--min-visits 2.0` (any value > 1.0) drops **every** key and exports an **empty blueprint**.

**Fix:** filter on the raw strategy-sum mass. Add a small accessor to `CompactRegretTable` (`crates/pkr-cfr/src/table.rs`, next to `get_average_strategy_slice`):

```rust
/// Raw, unnormalized reach-weighted strategy mass for one infoset.
/// This is the quantity `--min-visits` should threshold on.
pub fn strategy_sum_mass_of(&self, infoset_hash: u64) -> Option<f64> {
    let guard = self.hash_to_idx.pin();
    guard.get(&infoset_hash).map(|&idx| {
        let mut s = 0.0f64;
        for a in 0..K {
            s += self.load_sum(idx, a);
        }
        s
    })
}
```

and in `export_blueprint`:

```rust
if min_visits > 0.0 {
    keys.retain(|k| {
        table
            .strategy_sum_mass_of(*k)
            .is_some_and(|mass| mass >= min_visits as f64)
    });
}
```

(Also update the `--min-visits` help text to state the unit: reach-weighted strategy-sum mass, i.e. roughly Σ reach·σ over visits.)

---

### H3. Parallel subgame solve creates multiple aliasing `&mut Solver` — undefined behavior

**File:** `crates/pkr-subgame/src/lib.rs:807–831` (with `unsafe impl Send/Sync` at 560–561)

```rust
let this_addr = self as *mut Solver as usize;
(0..n_deals).into_par_iter().for_each(move |deal_idx| {
    let this: &mut Solver = unsafe { &mut *(this_addr as *mut Solver) };
    this.walk(root, deal_idx as u32, 1.0, 1.0, 0);
});
```

Every worker materializes a fresh `&mut Solver` **simultaneously**. Even though the *slots* touched by each deal are disjoint (`(node, deal)` indexing), multiple live `&mut` references to the same object are instant UB under the Rust aliasing model — LLVM is entitled to assume `&mut` parameters are `noalias`, and a future optimization pass can miscompile this. It will also fail any future `miri` run (the repo already wires miri in `ci/scripts/run-miri.sh` — but only on `pkr-core` + `pkr-cfr`, which is why this has never been caught).

**Fix (clean, no `unsafe`):** transpose the per-(node,deal) arrays to deal-major layout so each deal owns a contiguous slice, then use `par_chunks_mut`:

```rust
// Layout change:
//   reg0/deal-major: Vec<[f64; 6]> indexed deal_idx * n_nodes + node_id
let n = self.n_nodes;
self.reg0
    .par_chunks_mut(n)
    .enumerate()
    .for_each(|(deal_idx, reg0_deal)| {
        // pass reg1/sum0/sum1 deal-slices + &self view of tree/deals/term_val
        walk_deal(..., deal_idx, reg0_deal, ...);
    });
```

`walk` stops taking `&mut self` and instead takes the per-deal `&mut [f64;6]` slices plus `&self` for the immutable parts (tree, deals, term_val, cfg). The `unsafe impl Send/Sync` and the raw-pointer dance both disappear. `idx()` becomes `deal_idx * n_nodes + node_id`. The `nodes_visited`/`lazy_cache` atomics stay as-is.

If a full layout migration is too invasive right now, the minimal sound alternative is to make the four arrays `Vec<AtomicU64>`-backed f64 cells (like `strategy_sum` in `pkr-cfr::table`) and take `&self` in `walk` — but the deal-major transpose is both faster (contiguous per-deal writes) and simpler to reason about.

---

## 2. Medium severity

### M1. `PKR_SOFT_KMEANS=1` can never activate — gate compares street to 5

**File:** `crates/pkr-abstraction/src/lib.rs:631`

```rust
if !soft_kmeans_enabled() || street != 5 {
    return pkr_contracts::SoftHash::hard(primary);
}
```

Street codes in this codebase are `0..=3` (`Street::Preflop=0 … Street::River=3`; `traversal.rs:311` passes `current.street as u8`). `street != 5` is therefore always true, and the soft-assignment path is dead code. The `5` almost certainly came from confusing the street code with `board.len() == 5`. The feature (contract method `get_infoset_hash_soft`, `PKR_SOFT_KMEANS` env flag, `SOFT_BOUNDARY_FRAC`) silently does nothing.

**Fix:**

```rust
if !soft_kmeans_enabled() || street != (Street::River as u8) {
    return pkr_contracts::SoftHash::hard(primary);
}
```

…and add a test that builds a hand near a river tier boundary and asserts `SoftHash::is_soft()` is true when the flag is enabled (spawn the test in a subprocess or make the flag injectable, since it is `OnceLock`-cached).

### M2. `RiverResolver` is a broken solver shipped as public API

**File:** `crates/pkr-cfr/src/riversolve.rs`

The module doc claims *"near-solver river play … completes in milliseconds"*. Four independent defects:

1. **All bet actions have identical EV.** `action_ev` (lines 212–245) maps every `ActionKind::Bet(_)` to the `Call` branch (showdown for the current pot). No fold-equity, no extra money going in, no raise response — so actions 2/3/4/5 (the 0.5×, 1×, 2× sizings and the jam) are indistinguishable to the regret update. The "solve" degenerates to a fold-vs-play coin with noise on top.
2. **Wrong range construction.** `RiverResolver::new` (lines 74–88) excludes *all four* hole cards from *both* ranges. The opponent's range should exclude only the solver's own cards + board; as written, every enumerated villain combo is conditioned on the villain *not* holding the two cards they actually hold.
3. **Quadratic re-enumeration.** `update_regrets` (line 145) calls `self.villain_range.enumerate_combos(1700)` **inside** the hero loop — ~900 `Vec` allocations of ~900 pairs each, × 200 iterations. This is seconds-to-minutes per solve, not milliseconds.
4. **Semantic mismatch in `abstract_to_action`** (lines 178–210): builds `Bet(amount)` in "chips-to-add" terms (`stacks[actor].min(pot * 0.5)`), while the engine's `ActionKind::Bet` means "total street commitment" (see `state.rs` C1 comment). Any future caller that applies these actions to a real `GameState` gets silently wrong bet sizes.

It is currently dead code (only its own tests reference it), but it is `pub` from `pkr-cfr`, and unlike `public_br` (which carries an honest *"WIP — DO NOT WIRE"* banner), this module's doc *overclaims* correctness.

**Fix (recommended):** delete the module, or at minimum replace the doc claim with the same WIP banner `public_br.rs` uses, add `#[doc(hidden)]`, and fix the two mechanical issues so it stops being a trap:

```rust
// update_regrets: hoist the invariant enumeration out of the hero loop
let villain_combos = self.villain_range.enumerate_combos(1700);
for hero in &hero_combos {
    for villain in &villain_combos {
        ...
    }
}
```

A real fix of the EV model requires giving the opponent a fold/raise response model — out of scope for a patch; see §8 for the suggested approach (reuse the `pkr-subgame` public-tree solver instead of this stub).

### M3. GPU regret path would corrupt the table if ever enabled

**Files:** `crates/pkr-cfr/src/gpu.rs:86` vs `crates/pkr-cfr/src/table.rs:20–30`

The WGSL shader indexes a **stride-6 i32 layout with separate momentum array**:

```wgsl
let flat_idx = item.index * 6u + item.action;
```

The CPU table is **interleaved `[r0 m0 r1 m1 …]` i64 with stride 12** (`RM_STRIDE = K * RM_FIELDS`). The header comment in `table.rs` says the GPU path is "i32-only and unmaintained; it does NOT mirror the i64 table", and `flush_gpu_batch` is feature-gated — but it is still callable under `--features gpu`, writes through `store_rm`, and its test (`flush_writes_back_only_touched_entries…`) would silently validate against a *differently-shaped* memory region than production. A second latent bug in the same function: `let mut iteration = 0u32; … iteration = item.iteration;` takes the **last** item's iteration for the whole deduped batch even though a batch spans iterations (masked today because `dcfr_step` treats all t ≥ TAU nearly identically).

**Fix:** delete the module, or if it must stay:

```rust
#[cfg(feature = "gpu")]
compile_error!(
    "the gpu feature is broken against the i64 interleaved table (stride 12, \
     i64 cells); port gpu.rs to the current layout before enabling"
);
```

### M4. `run.sh` has no `set -e` and never checks cargo exit status — failed precompute flows into training

**File:** `run.sh:15` and `run.sh:118–144`

```bash
set -uo pipefail        # no -e
...
pre hand_ranks "$OUT/hand_ranks.bin"     # failure ignored
...
cargo run --release -p pkr-trainer -- ... 2>&1 | tee "$LOG"
echo "=== ${VERSION} complete at ... ==="  # printed even if cargo failed
```

`pre()` propagates cargo's status but no caller checks it. With `pipefail` the trainer pipeline's failure lands in `$?` — which nothing reads — and the script unconditionally prints "complete". A failed mid-precompute (disk full, OOM) then trains against stale `.bin` tables. `smoke.sh` gets this right (`set -euo pipefail`); the canonical launcher does not. (The `verify_artifacts.sh` audit at step 2 partially mitigates, but it only checks what already exists.)

**Fix:**

```bash
set -euo pipefail
...
pre hand_ranks "$OUT/hand_ranks.bin"
...
cargo run --release -p pkr-trainer -- ... 2>&1 | tee "$LOG"
```

With `-e`, every `pre` call and the training pipeline abort the script on non-zero status. If "keep going on cache hits" is desired, make it explicit inside `pre()` rather than by disabling `-e`.

### M5. `stats.json` records an eval seed that isn't the one used

**File:** `binaries/pkr-trainer/src/main.rs:1131` (vs `eval_seed_for`, lines 289–291)

`"eval_seed": EVAL_SEED` — but the actual eval seed is `eval_seed_for(done)` (the raw iteration number; the `EVAL_SEED ^ iter` scheme was retired by commit `18b6880`). Anyone reproducing a curve from `stats.json` will seed the BR walker differently than the recorded run. (The handoffs show how much pain seed-accounting causes in this project — this field actively lies.)

**Fix:** in the stats JSON, replace `"eval_seed": EVAL_SEED` with:

```rust
"eval_seed_scheme": "raw-iteration",
```

and delete the now-unused `EVAL_SEED` constant (or keep it with a `#[deprecated]`-style comment explaining it is historical).

### M6. `--save-best-reading` double-exports and can clobber the promoted blueprint

**File:** `binaries/pkr-trainer/src/main.rs:844–871`

In the gate-rejected branch, when the reading is a new raw minimum, the code exports the current table to **both** `cli.output` and `<stem>.best.bin`. Writing `.best.bin` is the point of the flag; also overwriting `cli.output` contradicts the flag's own doc ("Also export the blueprint on every new best READING **even when the sigma gate rejects promoting it**" — the main output is supposed to stay the promoted/significant model), and contradicts the `promoted` flag logic at line 1229, which assumes `cli.output` holds the last promoted model. A later end-of-run export then re-clobbers `cli.output` anyway, so the file churns between two different models during one run.

**Fix:** in the rejected branch, export only `best_path`; leave `cli.output` to the promotion path:

```rust
if cli.save_best_reading && is_new_raw_min(best_raw_mbb, br.exploitability_mbb) {
    let best_path = ...;
    export_blueprint(&trainer, &best_path, cli.min_visits, &fingerprint)?;
    best_raw_mbb = Some(br.exploitability_mbb);
}
```

---

## 3. Low severity

### L1. `history_signature_v3` hardcodes BB = 2.0

**File:** `crates/pkr-core/src/state.rs:718` — `let pot_bb = (self.street_start_pot / 2.0).max(1.0);`

`GameState::new(start_stack, sb, bb)` accepts arbitrary blinds; the v3 pot class is wrong for any `bb ≠ 2.0`. Flag-gated (`SIG_V3_SIZE_AWARE = false`) but a landmine. **Fix:** store `bb` on the state (one `f32` field set in `new()`), use `self.street_start_pot / self.bb.max(1e-6)`. Same for the magic `1.2` pot floors in `last_bet_fraction_bucket` (`state.rs:638`) and `action_bucket` (`pkr-core/src/abstraction.rs:75`) — derive them from `bb` (e.g. `1.2 * bb / 2`) or at least document the chip-scale assumption.

### L2. `evaluate_hand` panics on inputs with >7 unique valid cards

**Files:** `crates/pkr-eval/src/slow.rs:195–204`, `crates/pkr-eval/src/fast7.rs:75–82`

Both write into fixed `[u8; 7]` buffers with no bound on `idx`. Today every caller passes 2+5, but the `Evaluator` trait (`pkr-contracts`) doesn't document that, and a fuzz target or future caller passing 3 hole cards gets an index-out-of-bounds panic instead of an error. **Fix:** add `debug_assert!(hole.len() + board.len() <= 7)` at the top of both, and a doc note on the trait. (One line each.)

### L3. Combinadic unrank functions accept out-of-range indices silently

**File:** `crates/pkr-eval/src/lookup_fast.rs:8–107`

`combinadic_unrank_{2,3,5,6,7}` and `combinadic_unrank` return a *wrong combination* (not an error) for `index ≥ C(n, k)`; the loop just bottoms out. Callers today pass validated indices. **Fix:** `debug_assert!(index < choose(n, k))` in each, so debug builds catch contract violations.

### L4. `Solver` RNG seed has only ~52 effective values

**File:** `crates/pkr-subgame/src/lib.rs:650` — `seed: (cfg.root.board[0] as u64) ^ 0x5EED_2026_0000_0000`

All subgame solves on boards sharing the first board card share the sampled-river RNG stream. **Fix:** fold in more board entropy: `cfg.root.board[..board_len].iter().fold(0x5EEDu64, |h, &c| h.wrapping_mul(0x9E3779B97F4A7C15) ^ c as u64)`.

### L5. `br_walk` ignores `chance_enumerate_max_depth`

**File:** `crates/pkr-subgame/src/lib.rs:1090` vs `walk` at line 739

The solve walk honors the hybrid `full_chance && depth < chance_enumerate_max_depth` gate; the BR evaluation walk only checks `full_chance`. In hybrid mode the policy is fit on sampled deep rivers but evaluated on enumerated ones — not wrong (the BR *should* be exact), but the asymmetry makes the PKR_SUBGAME_CHANCE_DEPTH flag mean different things in the two walkers and silently changes BR cost. **Fix:** pass the same gate (or document why BR always enumerates).

### L6. `warn_nonfinite_regret_once` can never fire

**File:** `crates/pkr-cfr/src/table.rs:219–229` + `dcfr.rs`

The warning triggers on `new_r == i64::MAX`, but `update_regret_with_step` saturates via `saturating_add` *and* every flush clamps stored regrets to `±R_MAX = i64::MAX/4`, so `i64::MAX` is unreachable. Either detect saturation inside `update_regret_with_step` (return a flag) or delete the helper. As written it's dead assurance — the comment ("currently it cannot produce them") admits it.

### L7. `RangeTracker` fold actions skip the Bayesian update, undocumented

**File:** `crates/pkr-subgame/src/range_tracker.rs:158` — `if !matches!(action.kind, ActionKind::Fold) { self.update_range_for_action(...) }`

Blueprint fold probabilities carry information (junk hands fold more), so the correct posterior multiplies the folder's range by σ(fold | hand). The code deliberately skips folds but never says why. Impact today is ~zero (a fold ends the deal, so the folder's posterior is never consumed downstream), but any future use of `range(folder)` after a fold — e.g. showdow-range diagnostics — will read a biased distribution. **Fix:** either add the fold update or document the skip with its justification at the call site.

---

## 4. Code smells

| # | Where | Smell | Suggested fix |
|---|---|---|---|
| S1 | `crates/pkr-contracts/src/lib.rs:2`, `pkr-core/src/rules.rs:8` | `GameRules::max_actions_per_node()` returns **4**, but the real action space is up to 6 (fold + call + 3 sizings + jam). The trait method is used nowhere outside its own test. | Delete the method, or return `legal_actions_into`'s true bound (8) and actually use it to size buffers. |
| S2 | `crates/pkr-eval/src/lookup.rs:118–123` | `TableEvaluator::new` mmaps the same file **twice** (once directly, once inside `Fast7Evaluator`); the outer mmap serves only dead test helpers. | Keep one mmap; expose `Fast7Evaluator`'s, or pass the `Mmap` into `Fast7Evaluator::new`. |
| S3 | `crates/pkr-eval/src/lookup.rs:132`, `pkr-runtime/src/mmap.rs:147`, `pkr-abstraction/src/lib.rs:800`, `pkr-export/src/reader.rs:93` | Libraries print diagnostics via `eprintln!`. The trainer's stderr is parsed by scripts (`proftest.sh`, CI parsers). | Route through `tracing` (already a workspace dep) or return structured errors/warnings. |
| S4 | `crates/pkr-cfr/src/traversal.rs:381–385` | In the `avg_at_traverser=false` path the opponent infoset is looked up **twice** (`get_strategy_into` at 318, then `get_strategy_and_idx` at 381). | Compute `opp_idx = table.get_or_create_idx(hash)` once and reuse the already-masked `strategy`. |
| S5 | `crates/pkr-cfr/src/dcfr.rs:385` + `table.rs:652,679` | `DcfrStep` is documented as "precomputed … hoisted out of the per-group loop", but `flush_cpu_batch_with` recomputes `dcfr_step()` (two `powf`s) per group (sequential mode: per item). | Hoist once per flush: `let step = dcfr_step(max_iter)` / per distinct iteration in sequential mode, pass `&step` to `update_regret_with_step`. |
| S6 | `crates/pkr-core/src/state.rs:511–514` | Empty `if self.folded[next] { /* comment */ }` block. | Delete the block, keep the comment above the assignment. |
| S7 | `crates/pkr-core/src/state.rs:237–265, 301–332` | Bet-size dedup only guards the all-in; two `BET_SIZINGS` can clamp to the same min-raise-to and push duplicate actions. Harmless in the traverser (same bucket), but bots/harnesses see duplicated legal actions. | Track offered amounts like the all-in dedup does (`(b - raise).abs() < 1e-9`). |
| S8 | `crates/pkr-subgame/src/lib.rs:29–44` | `N_CLASSES` / `class_of` / `MIN_RANK` are dead; the header claims "Deals in the same class share CFR regrets", but regrets are strictly per-deal. | Delete the dead items or implement the class-sharing the doc promises; at minimum fix the doc. |
| S9 | `crates/pkr-subgame/src/lib.rs:415–462` | `build_tree`'s chance children include each player's hole cards; correctness depends on every consumer remembering to filter per deal (`walk` and `br_walk` do, `fill_blueprint_strat` visits them all). | Filter blocked cards when *building* the tree is impossible (tree is deal-independent), so instead: centralize the per-deal filter in one helper used by all three walkers, and add a debug assertion. |
| S10 | `crates/pkr-runtime/src/subgame.rs:174–178` | Opponent hand sampling seeded from `pot.to_bits()` only — identical pots always sample the identical 8 hands. | Mix in street/board/hole: `seed = pot.to_bits() ^ board_hash ^ our_hole_hash`. |
| S11 | `crates/pkr-cfr/src/lib.rs:77` | `iteration.fetch_add(n)` happens before the work; a panic mid-batch silently consumes iterations. | Advance the counter after the flush completes (`self.iteration.store(start_iter - 1 + n)`), or accept and document. |
| S12 | `crates/pkr-subgame/src/range_tracker.rs:443–497` | The fallback regression test **re-implements** the fallback loop instead of exercising `update_range_for_action`; the real code could regress and the test would stay green. | Refactor the fallback into a testable pure fn (`fn uniform_board_free(board) -> [f64; N_HANDS]`) and have both the tracker and the test call it. |
| S13 | `crates/pkr-fuzz/src/bin/arena.rs`, `tournament.rs` bin | (per handoff §3.4) table-directory assumption only *warns*; the fingerprint guard fires only if `PKR_CENTROID_FEATURE_V` is exported. | Promote the warning to an error unless `--allow-mixed-tables` is passed. |
| S14 | Shell scripts repo-wide | Mixed conventions: `smoke.sh` uses `set -euo pipefail`, `run.sh`/`bench.sh`/`fast.sh` do not; several scripts would benefit from `shellcheck` in CI. | Add `shellcheck` to the `fast.yml` gate and normalize on `set -euo pipefail`. |
| S15 | `crates/pkr-export/src/fmph.rs` + `pkr-runtime/src/fmph.rs` | Opt-in FMph tail is retained with a retry loop the writer itself calls "MINUTES to HOURS"; dead weight in two crates for a disabled feature. | Either time-box `build_fmph` and re-enable by default, or move to a `fmph` feature flag to keep it out of default builds. |

---

## 5. Documentation bugs (stale / self-contradictory comments)

| # | Location | Problem |
|---|---|---|
| D1 | `binaries/pkr-trainer/src/main.rs:270–296` | The doc blocks for `should_eval`, `is_new_raw_min`, and `eval_seed_for` are **fused into one jumbled comment** (an `#[inline]` sits mid-comment; `should_eval` ends up undocumented). Same failure class as the "mangled abstraction.rs patch" in the B-worklog and the "heredoc duplication" hazard. Restore three separate doc blocks. |
| D2 | `crates/pkr-cfr/src/traversal.rs:350–353` | "reach_prob at a traverser node is the *opponent's* reach" — false. Tracing the recursion (root 1.0, ×strategy at traverser nodes, unchanged at opponent nodes), `reach_prob` is the **traverser's own** reach; that's exactly why the average-strategy weight `strategy[a] * reach_prob * t^p` is correct. The code is right; the comment will mislead the next auditor. |
| D3 | `crates/pkr-abstraction/src/lib.rs:754–766` | River-quantization comment says "`>> 13` is a monotone quantization … ~1152 tiers … T2.2 uses >> 13 … (4x finer than >> 13)" while the code uses `RIVER_TIER_SHIFT = 15` (~288 tiers). Self-contradictory on three lines. State the shift once via the constant and delete the history. |
| D4 | `crates/pkr-core/src/state.rs:44–52` vs `abstraction.rs` fingerprint test | `sig_version` mapping includes v3 (`SIG_V3_SIZE_AWARE → 3`), but `fingerprint_tests::from_constants_captures_current_compile_time_values` still computes `expected_sig = if SIG_V2 {2} else {1}` — the test would fail the day v3 is switched on. Mirror the 3-way match. |
| D5 | `docs/experiments/training-nondeterminism.md` | The "FOURTH SOURCE (2026-10-02): alloc_idx race" and "OBSERVED: race fires in the seed-43 A/B" sections appear **twice each** (the heredoc-duplication hazard from the handoff, fossilized into a doc). Deduplicate. |
| D6 | `crates/pkr-contracts/src/lib.rs:73` | Typo "bluepints" in the FNV doc; harmless but it's the hash everything depends on — worth a tidy pass. |

---

## 6. Status of previously-known issues (verified against current code)

| Known issue | Status at `cce1339` | Evidence |
|---|---|---|
| `slow.rs` 6-card subset bug (B6) | **Fixed** | `COMBOS_6_5` table added; regression tests `six_card_inputs_see_every_subset` / `_do_not_panic`. |
| `TableProvider` zero-sum CDF mismatch (bug-hunt #2) | **Fixed** | `provider.rs` now mirrors `quantize_cdf` exactly, incl. the uniform fallback. |
| `mirror_to_seat0` missing dealer swap (bug-hunt #3) | **Fixed** | `m.dealer = 1 - m.dealer` at `subgame.rs:117` + tests. |
| Reader fingerprint / `max_actions_k` (turn-up #1, #2) | **Fixed** | `reader.rs:106–118` validates `k == ACTION_K` and `cdf == keys*k`. |
| Purify guard counting raw mass (turn-up #4) | **Fixed** | `survivors >= 2` at `writer.rs:37`. |
| Archetype shifter reading cumulative CDF (turn-up #3) | **Fixed (as documented)** | `public_br` honestly banners itself WIP; shifter path not wired. |
| F1 estimator opp-reach / `collect_cfv` threading | **Correct** (re-derived) | CFV accumulates `cv * deal_prior * opp_reach`; opponent recursion multiplies reach by `strat_masked[a]`; hook only fires on the br_seat=1 pass. |
| `alloc_idx` orphan race ("fourth source") | **Half-fixed, still open** | `snapshot.infosets` now reports `len()` (commit `9795867`), but `is_near_capacity`/CSV capacity remain timing-dependent, and the race still wastes capacity. **See §6.1** for the residual checkpoint-drift link. |
| 4-thread `train.ckpt` divergence (`strategy_sum_mass` ~1e-11) | **Open — root cause proposed in §6.1** | Doc says "source not yet localized". |
| `potential.rs` noise-variance bias | **Open by decision** | Documented in-module (conservative bias `0.25·q`); fine to keep. |
| `RuntimeSession::observe_*` swallowing tracker errors | **Open** | `let _ = t.apply_action(...)` at `session.rs:75,82`. Fix: return `Result<(), RangeError>` from `observe_action`/`observe_street` (callers may `let _ =` if they truly don't care) + `debug_assert!` in debug builds. |
| F3 size-aware signature / F4 potential features / F5 grid | **Gated by design** | Flags off; fingerprint axes wired. |

### 6.1 Proposed root cause for the still-open 4-thread checkpoint drift

The doc's open mystery: regret state is bit-identical across 4-thread runs, but `strategy_sum_mass` differs at ~5e-17 relative even though `apply_strategy_batch` sorts with the `prob.to_bits()` tiebreaker. Proposal that ties it to the (documented, still-live) **`alloc_idx` orphan race**:

1. When two workers race on `get_or_create_idx` for *different* hashes, the CAS on `next_idx` can hand out indices in either order → the **hash→idx assignment is permuted run-to-run**.
2. Every per-cell accumulation is still deterministic (one group per `(idx,action)` per batch, fixed fold order). But **diagnostics sum over cells in idx order** (`snapshot()` walks `0..allocated()`), and f64 addition is not associative — a permuted cell order changes the low bits of `strategy_sum_mass`, exactly the observed ~1e-11 over 339k cells.
3. The blueprint is unaffected because u8 quantization is far coarser than the drift — matching the doc's observation.

**Cheap fix:** make `snapshot().strategy_sum_mass` (and any other cross-cell aggregate) order-canonical by summing in *hash* order — iterate `hash_to_idx` (already sorted for checkpoints) instead of `0..allocated()`:

```rust
let guard = self.hash_to_idx.pin();
let mut keys: Vec<u64> = guard.iter().map(|(k, _)| *k).collect();
keys.sort_unstable();
let mut strat_mass = 0.0f64;
for k in &keys {
    let idx = *guard.get(k).unwrap();
    for a in 0..SUM_STRIDE {
        strat_mass += f64::from_bits(self.strategy_sum[idx * SUM_STRIDE + a].load(Ordering::Relaxed));
    }
}
```

**Full fix (also makes `train.ckpt` byte-reproducible at N threads):** after each dispatch's merge, allocate indices for newly-seen hashes in sorted-hash order on the coordinator thread (remove `alloc_idx` from the hot path; workers push `(hash, …)` and the coordinator assigns idx canonically). This eliminates both the orphan leak and the permutation in one move, at the cost of one sorted pass per batch — measure with the existing `flush_ns` instrumentation.

---

## 7. Test-coverage gaps observed

1. **No test exercises `blend_p0_strategy`'s missing-side branches** (that's how H1 survived). Add the two assertions from §H1.
2. **No test pins `--min-visits`** (H2). Add a trainer unit test: table with one 0.2-mass infoset and one 5.0-mass infoset → `min_visits=1.0` exports exactly the second.
3. **No test asserts `PKR_SOFT_KMEANS` changes anything** (M1) — flag-gated tests that self-skip are the pattern to avoid here.
4. **`miri` covers only `pkr-core` + `pkr-cfr`** — extend `run-miri.sh` to `pkr-subgame` (it would have caught H3 immediately; the arrays are big, so run with a tiny tree via `MIRIFLAGS="-Zmiri-disable-isolation"` on a unit-sized config).
5. **The export size test** (`v4_size_accounting_is_exact`) is good; consider the same for the checkpoint format (`PKRCKPT7` header = 8+4+40+4+4+8+8 bytes + map + arrays) so format drift is caught like blueprint drift is.
6. **Shell scripts are never linted** — add `shellcheck` to `fast.yml` (S14).

---

## 8. Suggested fix order

| Priority | Items | Rationale |
|---|---|---|
| 1 | H2, M4, M5, M6, D1 | Trainer correctness & operational safety; all small, mechanical, testable. |
| 2 | H1 (+test), L4, L5 | Subgame solver output integrity — the runtime's differentiating feature. |
| 3 | H3 (+§6.1 cheap fix), S8, S9 | Concurrency soundness before the next long training run; §6.1 full fix removes the last reproducibility gap. |
| 4 | M2 (delete or banner), M3 (delete or `compile_error!`), S1, S15 | Remove the two broken stubs and dead APIs so nobody wires them in by accident. |
| 5 | M1 (+test), L1–L7, S2–S7, S10–S14, D2–D6 | Hygiene sweep; each is a one-liner to a short function. |

---

## Appendix A — Quick-reference: all findings by file

| File | Findings |
|---|---|
| `binaries/pkr-trainer/src/main.rs` | H2, M5, M6, D1 |
| `crates/pkr-subgame/src/lib.rs` | H1, H3, L4, L5, S8, S9 |
| `crates/pkr-subgame/src/range_tracker.rs` | L7, S12 |
| `crates/pkr-cfr/src/riversolve.rs` | M2 |
| `crates/pkr-cfr/src/gpu.rs` | M3 |
| `crates/pkr-cfr/src/table.rs` | L6, §6.1 |
| `crates/pkr-cfr/src/dcfr.rs` | S5 |
| `crates/pkr-cfr/src/traversal.rs` | S4, D2 |
| `crates/pkr-cfr/src/lib.rs` | S11 |
| `crates/pkr-abstraction/src/lib.rs` | M1, D3 |
| `crates/pkr-core/src/state.rs` | L1, S6, S7, D4 |
| `crates/pkr-core/src/rules.rs` + `pkr-contracts` | S1, D6 |
| `crates/pkr-eval/src/slow.rs`, `fast7.rs` | L2 |
| `crates/pkr-eval/src/lookup.rs`, `lookup_fast.rs` | L3, S2, S3 |
| `crates/pkr-runtime/src/subgame.rs` | S10 |
| `crates/pkr-runtime/src/session.rs` | (known-open; fix in §6 table) |
| `run.sh` | M4 |
| `docs/experiments/training-nondeterminism.md` | D5, §6.1 |
| `.github/workflows`, `ci/scripts` | S13, S14, §7.4, §7.6 |
