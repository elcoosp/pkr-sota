# pkr-sota — Engine Performance & Quality Playbook

**Target codebase:** `pkr-sota` (dump.txt, ~16k lines, 11 crates) — head-up NLHE CFR trainer + mmap runtime.
**Audience:** an AI coding agent with shell + git access and no prior context.
**Method:** every change ships as a task with (1) unified diffs, (2) a verification gate, (3) a benchmarked A/B protocol against the previous binary. No change is "done" until its A/B passes.

---

## 0. HOW TO USE THIS DOCUMENT (READ FIRST — AGENT RULES)

You are implementing changes to a Rust workspace. Follow these rules exactly:

1. **Work in order.** Do Part II (harness) → Part III (correctness bundle) → Part IV (perf) → Part V (runtime) → Part VI (quality experiments). Later parts assume earlier gates pass.
2. **One task = one git branch = one A/B.** Branch name = task ID (e.g. `P1-c-cache`). Never mix two tasks in one measurement.
3. **Apply diffs with** `git apply --3way task.patch` (copy the hunk into a file), or edit manually: the `BEFORE`/`AFTER` blocks are authoritative if a hunk fails to apply.
4. **Never skip a Verification step.** `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace` (or `cargo test --workspace`) must pass before benchmarking.
5. **Hash-affecting changes invalidate blueprints.** Tasks marked `[HASH]` change infoset keys: old `blueprint.bin`, old checkpoints (`.ckpt`), and old precomputed artifacts that depend on them are dead. After a `[HASH]` task, regenerate artifacts and retrain from scratch. Tasks R0/R1/R2 are deliberately bundled so you pay **one** retrain, not three.
6. **A/B determinism.** Always benchmark with the same `--threads`, same machine, same precomputed artifacts, warm filesystem cache, and `--seed` fixed (Part II gives you `--seed`). Run A and B alternately, ≥3 rounds each, compare medians.
7. **The DNP list (Do Not Touch):**
   - Do not modify `FNV_OFFSET`/`FNV_PRIME` or `fnv1a` (on-disk format contract).
   - Do not "optimize" the WGSL shader in `gpu.rs` — it is dead code on the production path; if you touch `dcfr.rs` math, mirror it (see Task P1-b note).
   - Do not change `K = 6` action buckets in `table.rs` without a full retrain plan (checkpoint format, export format, runtime `max_actions_k` all depend on it).
   - Do not delete tests to make CI green.
   - Do not run A/B comparisons with different `--capacity` or different artifact files.
8. **Baseline numbers** (Mac Mini M1, 8 threads, k=64, from `docs/status.md`): ~27.7K it/s steady state, flush ≈ 30% of wall, cache_hit ≈ 0.92 @ 100K iters, eval = 21×5-card per 7-card call. Your job is to move these and prove it.

---

## 1. EXECUTIVE SUMMARY — FINDINGS RANKED BY ROI

Legend: **Type** = Q(uality)/P(erf)/R(untime)/T(ooling). **Effort** = S(<1h) / M(1–4h) / L(>4h). **Risk** = behavior change (B) or behavior-preserving (—). `[HASH]` = requires retrain + artifact regen.

| # | ID | Finding | Type | Expected gain | Effort | Risk |
|---|----|---------|------|---------------|--------|------|
| 1 | **R0** | `eval_5` ranks two-pair hands by their **lower** pair first (ascending `pairs[]` scan → low pair in dominant nibble). `99 over 55` loses to `TT over 66` incorrectly. Corrupts showdown payoffs, EHS, river buckets, `hand_ranks.bin`. | Q | Correct poker everywhere | S | B `[HASH]` |
| 2 | **R1** | Bet-bucket mapping defined **3× with 3 different definitions**: `traversal.rs` has an **unreachable branch** (`<1.5` before `<1.2` → bucket 3 never trained; 0.5x & 1.0x bets collide in bucket 2), `best_response.rs` uses edges 0.75/1.5, `writer.rs` anchors 0.45/0.9/2.2. Training, evaluation, and runtime translation all play different games. | Q | Large exploitability drop | S | B `[HASH]` |
| 3 | **R2** | River tiering `hand_rank >> 6` assumes 7462-scale ranks, but the evaluator emits **raw-bit** ranks (~8.34M span near 2³²) → **~130K river tiers** instead of the intended ~117. Infoset explosion at the river; each river infoset barely revisited. Fix = dense 7462-scale ranks. | Q | Smaller table, faster convergence | M | B `[HASH]` |
| 4 | **P1-a** | 7-card eval = min over **21** 5-card evals (each = sort + combinadic + mmap probe). Replace with direct bitboard 7-card evaluator → ~40–80ns vs ~3µs. Speeds terminal payoffs, river bucketing, EHS, and turns a multi-hour turn-table precompute into minutes. | P | +20–40% it/s; precompute 10–30× | M | — |
| 5 | **T0** | No determinism (`SmallRng::seed_from_u64(rand::random())`), no A/B harness, no exploitability in the profiling loop, bench scripts missing `target-cpu=native`. You cannot measure anything honestly until this lands. | T | enables everything | S | — |
| 6 | **P1-b** | `flush_cpu_batch` calls `update_regret_pfr_plus` per group → 2× `powf` + 1× `sqrt` **per group**. Hoist to batch level; compute DCFR discount in **f64** so the discount actually applies past t≈10K (f32 saturation makes DCFR = vanilla CFR today). | P+Q | +2–4% it/s; real discounting | S | B(mild) |
| 7 | **P1-c** | Flush path dominated by `par_sort` on tuple keys. Pack `(index, action)` into one `u64` key; accumulate group deltas in f64. | P | +5–10% it/s | S | — |
| 8 | **P1-d** | Thread-local idx cache is a `HashMap` with a **clear-at-1M-entries cliff** (hit rate collapses above 1M infosets). Replace with direct-mapped power-of-2 cache: no cliff, no hashing on hit. | P | +5–15% it/s late-run; removes cliff | S | — |
| 9 | **P1-e** | `history_signature()` rescans the whole action history per node (O(depth)). Maintain `raises_total` incrementally → O(1). | P | +2–4% it/s | S | — |
| 10 | **P1-f** | `[[0usize; 10]; 6]` zero-init per node visit ≈ 480 B memset/node ≈ GBs/s of wasted memset. Replace with an 8-entry bucket-tag array. | P | +2–5% it/s | S | — |
| 11 | **P2-a** | Checkpoint save does per-element atomic load + 4-byte write over ~500 MB. `AtomicI32`→`i32` slice cast + single `write_all` → 10–50× faster checkpoints. | P | checkpoint UX | S | — |
| 12 | **P3-a** | Runtime lookup: branchy binary search + `from_le_bytes` per probe + bounds checks. Branchless search over `bytemuck`-cast `&[u64]` → 2–3× lookup speedup, fewer cache misses. | R | p99 lookup ↓ | S | — |
| 13 | **P3-b** | FMph (MPHF) is built by the exporter but **never used** by the runtime. Wire it behind a header flag: O(1) lookup + one verification read. | R | O(1) lookup | M | B(format-add) |
| 14 | **Q1** | Once ranks are dense (R2): sweep river tiers (`>> 5` / `>> 6` / `>> 7`) and board-bucket mixing. One-line changes, big A/B surface. | Q | blueprint strength | S | B `[HASH]` |
| 15 | **Q2** | Strategy-sum has no discounting (γ-discount is dead code) and no recency weighting. Lazy per-infoset decay via last-touch iteration. | Q | better late-run play | M | B `[HASH]`(ckpt v5) |
| 16 | **Q3** | Sizing ladder is fixed {0.5, 1.0, 2.0}×pot + all-in, `MAX_RAISES_PER_STREET=3`. Make the ladder a constant; A/B alternatives (0.33/0.66/1.33; 4 raises). | Q | blueprint strength | S | B `[HASH]` |
| 17 | **Q4** | Turn precompute enumerates C(52,6)×15 = 305.4M entries with MC-EHS each — the real-run pipeline (`run.sh` step 7/8) is **days of compute** as written. P1-a fixes most of it; a texture-based two-level turn abstraction is the structural fix. | P+Q | pipeline feasibility | L | — |
| 18 | **T1** | Startup validation of abstraction table sizes (fail fast instead of silent 100× MC fallback), Kuhn exploitability CI gate, criterion micro-benches, fuzz-harness card-dup fix. | T | regression safety | M | — |

**The single most important sentence in this document:** the engine currently mis-ranks two-pair hands (R0), trains a bucket that its own evaluator then ignores (R1), and slices the river ~1000× finer than its design comment claims (R2). Fix these three before optimizing anything else, because every benchmark you run before them measures a corrupted objective.

---

## 2. SYSTEM ANATOMY — WHERE TIME GOES

Hot path per logical iteration (two traversals, one per seat, same deal):

```
run_iterations_parallel (pkr-cfr/src/lib.rs)
└─ per 16-iter chunk, per iter:
   ├─ partial shuffle (9 cards)                      ~negligible
   ├─ traverse() recursion (~255–306 nodes total, depth ≈ 8.1)
   │  ├─ history_signature()          ← O(history_len) scan per node      [P1-e]
   │  ├─ abstraction.get_infoset_hash()
   │  │  ├─ flop/turn: mmap table index (fast)
   │  │  └─ river: evaluator.evaluate_hand (21× eval_5!) ← hot             [P1-a]
   │  ├─ IDX_CACHE lookup (HashMap, TLS) or papaya map                    [P1-d]
   │  ├─ get_strategy_and_idx → 6 atomic loads + normalize
   │  ├─ legal_actions_into + bucket tagging (480 B zero-init)             [P1-f]
   │  └─ terminal: terminal_payoff → 2× evaluate_hand (21× eval_5!)        [P1-a]
   └─ push BatchItem/StrategyOp into thread-local Vecs
├─ merge all chunk Vecs (memcpy)
├─ apply_strategy_batch: retain + par_sort + group + CAS f64 add
└─ flush_cpu_batch: par_sort + group + per-group powf×2 + sqrt           [P1-b, P1-c]
```

Measured (8 threads): traverse ≈ 60–70%, flush ≈ 30% of wall. The evaluator alone is likely 20–35% of traverse (every river node + every terminal). That is why R0/P1-a come before micro-tuning the flush.

Quality chain: `eval_5` (slow.rs) → `hand_ranks.bin` (precompute) → `TableEvaluator` → river tiers + terminal payoffs + EHS → infoset hashes → regrets → exported CDF → `best_response.rs` (exploitability) and runtime advice. **A bug at the left end poisons everything to the right.** That is exactly what R0/R1/R2 are.

---

## PART II — BENCHMARK & A/B HARNESS (build this FIRST)

### Task T0-a — Deterministic training seed `[no behavior change]`

**Why.** `crates/pkr-cfr/src/lib.rs::run_iterations_parallel` seeds each chunk with `rand::random::<u64>()`. Two runs of the same binary produce different regrets, so A/B deltas are polluted by run-to-run variance. With a fixed seed, same thread count, and the f64 group-sum fix from P1-c, runs are reproducible to within float noise.

**Diff 1 — `binaries/pkr-trainer/src/main.rs`** (add the CLI arg after `eval_deals`):

```diff
     /// Deals sampled per exploitability check. Accuracy ~ 1/sqrt(deals).
     #[arg(long, default_value_t = 2000)]
     eval_deals: u32,
+
+    /// Base RNG seed for traversal. Same seed + same thread count gives
+    /// reproducible runs (required for A/B benchmarking).
+    #[arg(long, default_value_t = 42)]
+    seed: u64,
```

and inside the training loop:

```diff
-        trainer.run_iterations_parallel(batch as usize);
+        trainer.run_iterations_parallel(batch as usize, cli.seed);
```

**Diff 2 — `crates/pkr-cfr/src/lib.rs`**:

```diff
-    pub fn run_iterations_parallel(&mut self, n: usize) {
+    pub fn run_iterations_parallel(&mut self, n: usize, seed: u64) {
```

```diff
-                    let mut rng = SmallRng::seed_from_u64(rand::random::<u64>());
+                    let mut rng = SmallRng::seed_from_u64(
+                        seed ^ (chunk_idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
+                    );
```

```diff
     pub fn run_iteration_parallel(&mut self) {
-        self.run_iterations_parallel(1);
+        self.run_iterations_parallel(1, 42);
     }
```

**Verify.** `cargo build --release -p pkr-trainer` then run twice with `--iterations 2048 --threads 4 --seed 7` plus `--stats-json` and diff the two `sample_infosets` arrays in `stats.json` — identical or near-identical (exact identity requires P1-c's f64 group sum; document any last-bit drift, do not chase it).

### Task T0-b — `ab_test.sh` A/B driver `[new file]`

**Why.** One command that trains A and B binaries alternately under identical conditions and prints a medians table. Alternation absorbs machine-state drift (thermal, cache).

**Create `ab_test.sh` in the repo root:**

```bash
#!/usr/bin/env bash
# A/B throughput + exploitability harness.
# Usage: ./ab_test.sh <DIR_A> <DIR_B> [seconds_per_run] [rounds] [threads]
#   DIR_A: baseline repo checkout (or worktree)
#   DIR_B: candidate checkout
# Both dirs must contain the repo. Artifacts are shared from DIR_A/.smoke.
set -euo pipefail

A_DIR="$(cd "$1" && pwd)"
B_DIR="$(cd "$2" && pwd)"
SECS="${3:-20}"
ROUNDS="${4:-3}"
THREADS="${5:-8}"
ART="$A_DIR/.smoke"

[ -f "$ART/turn_abstraction.bin" ] || { echo "Run ./smoke.sh in $A_DIR first"; exit 1; }

build() { (cd "$1" && cargo build --release -p pkr-trainer 2>&1 | tail -1); }
BIN_A="$A_DIR/target/release/pkr-trainer"
BIN_B="$B_DIR/target/release/pkr-trainer"
build "$A_DIR"; build "$B_DIR"

run_once() {  # $1 = binary, $2 = seconds -> echoes "it/s expl"
    local bin="$1"
    local out
    out=$("$bin" --bench-seconds "$2" --threads "$THREADS" --seed 42 \
        --capacity 10000000 --report-every 50000 \
        --centroids "$ART/centroids.bin" \
        --preflop-table "$ART/preflop_abstraction.bin" \
        --flop-table "$ART/flop_abstraction.bin" \
        --flop-buckets "$ART/flop_buckets.bin" \
        --turn-table "$ART/turn_abstraction.bin" \
        --river-table "$ART/river_buckets.bin" \
        --rank-table "$ART/hand_ranks.bin" \
        --output "$ART/ab_blueprint.bin" 2>&1 || true)
    local rate
    rate=$(echo "$out" | grep -oE '[0-9]+\.[0-9] it/s' | tail -1 | cut -d' ' -f1)
    echo "${rate:-0}"
}

A_RATES=(); B_RATES=()
for r in $(seq 1 "$ROUNDS"); do
    echo "== round $r: A then B =="
    A_RATES+=("$(run_once "$BIN_A" "$SECS")")
    B_RATES+=("$(run_once "$BIN_B" "$SECS")")
    echo "  A: ${A_RATES[$(( ${#A_RATES[@]} - 1 ))]} it/s | B: ${B_RATES[$(( ${#B_RATES[@]} - 1 ))]} it/s"
done

median() { printf '%s\n' "$@" | sort -g | awk '{a[NR]=$1} END {print (NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2}'; }
A_MED=$(median "${A_RATES[@]}"); B_MED=$(median "${B_RATES[@]}")
echo ""
echo "=== RESULT (median of $ROUNDS, ${SECS}s each, ${THREADS} threads) ==="
printf "A (baseline):  %s it/s\nB (candidate): %s it/s\nDelta: %+.1f%%\n" \
    "$A_MED" "$B_MED" "$(awk -v a="$A_MED" -v b="$B_MED" 'BEGIN{print (b-a)/a*100}')"
# Verdict rule: require >= 2% median improvement to accept a perf-only task.
```

`chmod +x ab_test.sh`. For **candidate branches**, create a worktree: `git worktree add ../pkr-b <branch>` — binaries then live in separate `target/` dirs (cold first build; warm after).

**Quality A/B:** for exploitability comparisons use `proftest.sh`-style runs (Task T0-c) and compare `EVAL ... expl_mbb=` lines at identical iteration checkpoints with identical seeds/deal counts.

### Task T0-c — Wire exploitability + native codegen into the profile loop `[edit 2 files]`

**Why.** `proftest.sh` trains 100K iterations but never calls `--eval-every`, so quality regressions are invisible. `bench.sh`/`proftest.sh` also lack `target-cpu=native` (only `run.sh` has it), so their numbers understate production throughput.

**Diff — `proftest.sh`** (top of file, after `cd "$(dirname "$0")"`):

```diff
 PROF_DIR="${PROF_DIR:-./outputs/v0-proftest}"
+export RUSTFLAGS="-C target-cpu=native"
```

and in the training invocation, after `--report-every 5000`:

```diff
     --report-every 5000 \
+    --eval-every "${EVAL_EVERY:-25000}" \
+    --eval-deals "${EVAL_DEALS:-20000}" \
     --centroids "$PROF_DIR_ABS/centroids.bin" \
```

**Diff — `bench.sh`** (after `cd "$(dirname "$0")"`):

```diff
 BENCH_DIR="${BENCH_DIR:-./.smoke}"
+export RUSTFLAGS="-C target-cpu=native"
```

**Diff — create `.cargo/config.toml`** (repo root) so all cargo builds match production:

```toml
[build]
rustflags = ["-C", "target-cpu=native"]
```

> Note: `run.sh` already sets `RUSTFLAGS` in-process; env `RUSTFLAGS` overrides `.cargo/config.toml` — values agree, so no conflict. Binaries become machine-specific: never ship them cross-machine.

**Verify.** `./proftest.sh` prints at least two `EVAL iter=... expl_mbb=...` lines and CSV rows gain nothing (schema unchanged). Record the baseline `expl_mbb` — every quality task in this document is judged against it.

### Task T0-d — Baseline data sheet `[manual, 10 min]`

Before touching anything else, run and record (append to `docs/baseline.md`):

```bash
./smoke.sh                      # artifact sanity
./ab_test.sh . . 20 3 8         # A vs A: must show ~0% delta (harness sanity)
./proftest.sh                   # 100K iters + EVAL lines + stats.json
cargo run --release -p pkr-testgames --bin kuhn-experiment
```

Record: median it/s (A-vs-A should agree within ±2%), final `expl_mbb`, Kuhn final exploitability per variant, infoset count, `uniform_fallback` from `stats.json` (expect it to be large pre-R1 — that is the bucket-2 collision signature).


## PART III — CORRECTNESS BUNDLE (R0 + R1 + R2, ship together, ONE retrain)

These three tasks all change infoset semantics or the rank space, so they share one artifact regeneration + one retrain. Implement them on one branch `R-bundle`, gate it, retrain, then A/B the bundle against the Part II baseline.

---

### Task R0 — Fix two-pair ranking in `eval_5` `[HASH]` — the worst bug in the repo

**Evidence.** `crates/pkr-eval/src/slow.rs`, two-pair block:

```rust
let pairs: Vec<usize> = rank_counts
    .iter()
    .enumerate()
    .filter(|&(_, &c)| c == 2)
    .map(|(i, _)| i)
    .collect();
if pairs.len() >= 2 {
    let p1 = pairs[0] as u8;   // ← pairs[] is ascending: this is the LOW pair
    let p2 = pairs[1] as u8;   // ← and this is the HIGH pair
    ...
    let raw = (2u32 << 20) | ((p1 as u32) << 16) | ((p2 as u32) << 12) | ((kicker as u32) << 8);
```

`raw` is compared as `!raw` (lower = better), and the **dominant nibble carries `p1`, the low pair**. Poker compares the high pair first. Concrete counterexample (both hands two pair, kicker K, no flush/straight possible):

- Hand A = `99 55 K` → pairs {5s(idx 3), 9s(idx 7)} → encoded (3, 7, K)
- Hand B = `TT 66 K` → pairs {6s(idx 4), Ts(idx 8)} → encoded (4, 8, K)

Poker truth: B beats A (T over 6 > 9 over 5). Engine: compares (3,·) < (4,·) → A "wins". Every showdown between differently-leveled two-pair hands is mis-scored; EHS, river buckets, and `hand_ranks.bin` inherit it. (For 5-card inputs the *class* detection is correct; only the intra-class nibble order of two pair is wrong. Quads/trips/pair/high-card encode correctly.)

**Diff — `crates/pkr-eval/src/slow.rs`:**

```diff
     if pairs.len() >= 2 {
-        let p1 = pairs[0] as u8;
-        let p2 = pairs[1] as u8;
+        // R0 FIX: pairs[] is ascending by rank index (deuce=0 .. ace=12).
+        // Poker compares the HIGH pair first, so the highest pair must take
+        // the dominant nibble (<<16) and the second pair the next nibble.
+        let p1 = pairs[pairs.len() - 1] as u8;
+        let p2 = pairs[pairs.len() - 2] as u8;
         let kicker = ranks
             .iter()
             .find(|&&r| r != p1 && r != p2)
             .copied()
             .unwrap_or(0);
```

**Add a regression test** — create a `tests` module at the bottom of `slow.rs` (the file has none today):

```rust
#[cfg(test)]
mod r0_tests {
    use super::*;

    #[test]
    fn two_pair_compares_high_pair_first() {
        // Card = suit*13 + rank (rank idx: 2=0, 5=3, 6=4, 8=6, 9=7, T=8, K=11).
        // A = 99 55 K  => cards: 9s=7, 9h=20, 5d=29, 5c=42, Kh=24
        let a = [7u8, 20, 29, 42, 24];
        // B = TT 66 K  => cards: Ts=8, Th=21, 6d=30, 6c=43, Kh=24
        let b = [8u8, 21, 30, 43, 24];
        // Poker: B (tens over sixes) beats A (nines over fives).
        // eval_5 returns !raw (lower = better), so eval(B) must be < eval(A).
        let ra = eval_5(&a);
        let rb = eval_5(&b);
        assert!(rb < ra, "TT+66 (b={rb}) must rank better than 99+55 (a={ra})");
    }

    #[test]
    fn aces_up_beats_kings_up() {
        // A = AA 22 K  (pairs: 2s=0, As=12, kicker K)
        let a = [12u8, 25, 26, 39, 24];
        // B = KK QQ K  (pairs: Qs=10, Ks=11, kicker K)
        let b = [11u8, 24, 23, 36, 24];
        let ra = eval_5(&a);
        let rb = eval_5(&b);
        assert!(ra < rb, "AA+22 (a={ra}) must rank better than KK+QQ (b={rb})");
    }
}
```

**Latent hardening (same diff, no behavior change for 5-card inputs).** `trips`/`pair` are located with an ascending `.position(...)` scan. With 5 cards there is at most one trips rank and one pair rank, so results are identical today — but the function is a foot-gun if anyone ever feeds it more cards. Make the intent explicit:

```diff
-    let trips = rank_counts.iter().position(|&c| c == 3);
-    let pair = rank_counts.iter().position(|&c| c == 2);
+    // Scan from the top rank down: highest trips/pair wins if several exist.
+    // (5-card inputs have at most one of each - no behavior change.)
+    let trips = rank_counts.iter().rposition(|&c| c == 3);
+    let pair = rank_counts.iter().rposition(|&c| c == 2);
```

> ⚠️ After this fix, `hand_ranks.bin` (and every derived artifact) is stale because the raw encoding of two-pair hands changed. Regenerate in the R-bundle gate below.

**Verify.** `cargo test -p pkr-eval` — both new tests pass; all pre-existing eval tests still pass.

---

### Task R1 — One canonical bet-bucket mapping (kill the unreachable branch) `[HASH]`

**Evidence.** Three definitions disagree:

| Source | Bucket 2 (small) | Bucket 3 (medium) | Bucket 4 (large) | Pot clamp |
|---|---|---|---|---|
| `pkr-cfr/src/traversal.rs::abstract_action_index` | `fraction < 1.5` | **UNREACHABLE** (`< 1.2` after `< 1.5`) | else | `max(1.2)` |
| `pkr-core/src/state.rs::abstract_action_index_static` | same dead-branch bug | same | same | `max(1.2)` |
| `pkr-exploit/src/best_response.rs::bucket_of` | `< 0.75` | `< 1.5` | else | `max(1.0)` |
| `pkr-export/src/writer.rs::STREET_BET_FRACTIONS` (translation anchors) | 0.45 | 0.9 | 2.2 | — |

Consequences: (a) the generated concrete sizes are 0.5/1.0/2.0×pot + all-in; with the training-side mapping both 0.5× and 1.0× land in bucket 2 and **bucket 3 is never trained** — every infoset carries a permanently-uniform bucket-3 strategy and only 5 of 6 regret lanes are used; (b) the exploitability walker splits 0.5×/1.0× into buckets 2/3 — i.e., it evaluates against strategies for a bucket split training never learned; (c) runtime translation anchors don't match trained sizes.

**Canonical decision (do not deviate):** buckets 0=fold, 1=check/call, 2=small (<0.75×pot), 3=medium (<1.5×pot), 4=large (≥1.5×pot, non-all-in), 5=all-in. Then 0.5→2, 1.0→3, 2.0→4, all-in→5 — matching the `best_response.rs` test that already asserts exactly this. Translation anchors become 0.5/1.0/2.0.

**Diff 1 — `crates/pkr-core/src/state.rs`** — replace the private mapper with the shared canonical one:

```diff
-/// Map action kind to abstract bucket (0..5) given the state before the action.
-fn abstract_action_index_static(kind: &ActionKind, state: &GameState) -> u8 {
-    match kind {
-        ActionKind::Fold => 0,
-        ActionKind::Check | ActionKind::Call => 1,
-        ActionKind::Bet(amount) => {
-            let pot = state.pot.max(1.2);
-            let fraction = amount / pot;
-            if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
-                5 // all-in
-            } else if fraction < 1.5 {
-                2
-            } else if fraction < 1.2 {
-                3
-            } else {
-                4
-            }
-        }
-    }
-}
+/// CANONICAL action-bucket mapping (R1). Single source of truth shared by:
+///   - pkr-cfr traversal (strategy/regret buckets)
+///   - this module's abstract_history recording
+///   - pkr-exploit best-response walker
+///   - pkr-export translation anchors (STREET_BET_FRACTIONS)
+///
+/// Buckets: 0=Fold, 1=Check/Call, 2=Small(<0.75x pot), 3=Medium(<1.5x),
+///          4=Large(>=1.5x), 5=All-in.
+/// Concrete ladder 0.5/1.0/2.0 x pot maps 1:1 onto buckets 2/3/4.
+pub fn abstract_action_index(kind: &ActionKind, state: &GameState) -> usize {
+    match kind {
+        ActionKind::Fold => 0,
+        ActionKind::Check | ActionKind::Call => 1,
+        ActionKind::Bet(amount) => {
+            let pot = state.pot.max(1.0);
+            let fraction = amount / pot;
+            if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
+                5 // all-in
+            } else if fraction < 0.75 {
+                2
+            } else if fraction < 1.5 {
+                3
+            } else {
+                4
+            }
+        }
+    }
+}
```

and inside `apply_action_internal`, update the recording call:

```diff
-        let bucket = abstract_action_index_static(&action.kind, self);
+        let bucket = abstract_action_index(&action.kind, self) as u8;
```

**Diff 2 — `crates/pkr-cfr/src/traversal.rs`** — delete the local copy and use the shared one:

```diff
 use crate::gpu::BatchItem;
 use crate::metrics::LocalMetrics;
 use pkr_contracts::{AbstractionBuilder, Evaluator};
-use pkr_core::state::{Action, ActionKind, GameState, Street};
+use pkr_core::state::{abstract_action_index, Action, GameState, Street};
 use rand::RngExt;
 use rand::Rng;
```

```diff
-fn abstract_action_index(kind: &ActionKind, state: &GameState) -> Option<usize> {
-    match kind {
-        ActionKind::Fold => Some(0),
-        ActionKind::Check | ActionKind::Call => Some(1),
-        ActionKind::Bet(amount) => {
-            let pot = state.pot.max(1.2);
-            let fraction = amount / pot;
-            if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
-                Some(5)
-            } else if fraction < 1.5 {
-                Some(2)
-            } else if fraction < 1.2 {
-                Some(3)
-            } else {
-                Some(4)
-            }
-        }
-    }
-}
-
```

and in `traverse`, the bucketing loop becomes total (every legal action has a bucket):

```diff
     let mut action_counts = [0usize; K];
     let mut action_indices = [[0usize; 10]; K];
     for (idx, action) in num_actions.iter().enumerate() {
-        if let Some(a) = abstract_action_index(&action.kind, current) {
-            if action_counts[a] < 10 {
-                action_indices[a][action_counts[a]] = idx;
-                action_counts[a] += 1;
-            }
+        let a = abstract_action_index(&action.kind, current);
+        if action_counts[a] < 10 {
+            action_indices[a][action_counts[a]] = idx;
+            action_counts[a] += 1;
         }
     }
```

(If you implement P1-f first this loop is restructured anyway — apply R1 first, then P1-f rewrites it.)

**Diff 3 — `crates/pkr-exploit/src/best_response.rs`** — replace the local mapper body (keep the wrapper so call sites/tests compile):

```diff
 /// Which abstract bucket a concrete action maps to.
-/// Mirrors traversal.rs::abstract_action_index.
+/// Delegates to the canonical mapping in pkr-core (R1): training, export and
+/// exploitability now share one definition.
 fn bucket_of(kind: &ActionKind, state: &GameState) -> Option<usize> {
-    match kind {
-        ActionKind::Fold => Some(0),
-        ActionKind::Check | ActionKind::Call => Some(1),
-        ActionKind::Bet(amount) => {
-            let pot = state.pot.max(1.0);
-            let fraction = amount / pot;
-            if *amount >= state.stacks[state.actor] + state.street_bets[state.actor] {
-                Some(5)
-            } else if fraction < 0.75 {
-                Some(2)
-            } else if fraction < 1.5 {
-                Some(3)
-            } else {
-                Some(4)
-            }
-        }
-    }
+    Some(pkr_core::state::abstract_action_index(kind, state))
 }
```

(Its existing test `bucket_mapping_injective_for_canonical_sizes` keeps passing — it asserts exactly the canonical edges.)

**Diff 4 — `crates/pkr-export/src/writer.rs`** — anchors now equal trained sizes:

```diff
 pub const STREET_BET_FRACTIONS: [[f32; 6]; 4] = [
     [0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
-    [0.0, 0.0, 0.45, 0.9, 2.2, 1.0],
-    [0.0, 0.0, 0.45, 0.9, 2.2, 1.0],
-    [0.0, 0.0, 0.45, 0.9, 2.2, 1.0],
+    [0.0, 0.0, 0.5, 1.0, 2.0, 1.0],
+    [0.0, 0.0, 0.5, 1.0, 2.0, 1.0],
+    [0.0, 0.0, 0.5, 1.0, 2.0, 1.0],
 ];
```

**Add one consistency test** — in `crates/pkr-cfr/src/traversal.rs` tests module (it exists, `mod tests`):

```rust
    #[test]
    fn r1_bucket_mapping_is_canonical() {
        use pkr_core::state::{abstract_action_index, ActionKind};
        let s = GameState::new(200.0, 1.0, 2.0);
        let pot = 3.0f32;
        assert_eq!(abstract_action_index(&ActionKind::Fold, &s), 0);
        assert_eq!(abstract_action_index(&ActionKind::Check, &s), 1);
        assert_eq!(abstract_action_index(&ActionKind::Call, &s), 1);
        assert_eq!(abstract_action_index(&ActionKind::Bet(pot * 0.5), &s), 2);
        assert_eq!(abstract_action_index(&ActionKind::Bet(pot * 1.0), &s), 3);
        assert_eq!(abstract_action_index(&ActionKind::Bet(pot * 2.0), &s), 4);
        assert_eq!(abstract_action_index(&ActionKind::Bet(200.0), &s), 5);
    }
```

**Verify.** Full workspace test run. Then check `stats.json` after a 100K-iter proftest: `dominant_action_counts` should now show meaningful mass on **four** action lanes (fold/call/small/medium/large) instead of three, and bucket-3 `uniform_fallback` mass should drop sharply.

---

### Task R2 — Dense 7462-scale hand ranks (fix river tiering) `[HASH]`

**Evidence.** `eval_5` returns `!raw` where `raw` packs class+kickers into 21 bits starting at bit 20 — so all valid ranks live in a band roughly `[4.2858e9, 4.2941e9]` (span ≈ 8.34M). The river tier in `pkr-abstraction/src/lib.rs`:

```rust
let hand_rank = self.evaluator.evaluate_hand(hole, board) as u64;
let hand_bucket = hand_rank >> 6;
```

The comment says "hand rank has cardinality 7462 … `>> 6` gives 116 tiers" — true only for *dense* 1..7462 ranks. On the raw scale, `>> 6` yields ~130,000 distinct tiers. The river infoset space is ~1000× finer than designed: the map balloons and each river infoset is barely revisited, which is exactly where convergence quality dies.

**Fix shape.** Keep raw encoding inside evaluators (bit-compatible with `hand_ranks.bin`), and add a dense mapping **layer** in `TableEvaluator` built at load time from the file. River code needs zero changes — after the fix, `hand_rank >> 6` genuinely gives 117 tiers.

**Diff 1 — new file `crates/pkr-eval/src/dense.rs`:**

```rust
//! Dense rank mapping (R2): raw `!raw`-encoded hand classes -> 1..=7462
//! poker ranks (7462 = best). Built at load time from hand_ranks.bin.
//!
//! All 5-card hand classes have distinct raw encodings; the file holds the
//! raw rank of each of the C(52,5) = 2,598,960 hands. Sorting + dedup
//! yields exactly the 7462 equivalence classes, ascending (best first,
//! because raw is inverted: lower = better).

pub struct DenseRankLut {
    min_raw: u32,
    lut: Vec<u16>, // lut[i] = dense rank of raw (min_raw + i); 0 = impossible
}

impl DenseRankLut {
    pub fn from_raw_table(raws: &[u32]) -> Self {
        let mut sorted: Vec<u32> = raws.to_vec();
        sorted.retain(|&r| r != u32::MAX);
        sorted.sort_unstable();
        sorted.dedup();
        assert!(
            sorted.len() <= u16::MAX as usize,
            "dense rank does not fit u16: {} classes",
            sorted.len()
        );
        let min_raw = *sorted.first().expect("empty rank table");
        let max_raw = *sorted.last().unwrap();
        let span = (max_raw - min_raw) as usize + 1;
        let mut lut = vec![0u16; span];
        // v[0] = best hand => dense rank 7462. Rank counts DOWN as raw grows.
        let n = sorted.len() as u16;
        for (i, &raw) in sorted.iter().enumerate() {
            lut[(raw - min_raw) as usize] = n - (i as u16);
        }
        Self { min_raw, lut }
    }

    /// Dense poker rank, 1 (worst) ..= class_count (best).
    #[inline(always)]
    pub fn rank(&self, raw: u32) -> u32 {
        debug_assert!(raw >= self.min_raw);
        let idx = (raw - self.min_raw) as usize;
        debug_assert!(idx < self.lut.len());
        self.lut[idx] as u32
    }

    pub fn class_count(&self) -> u32 {
        self.lut.iter().filter(|&&v| v != 0).count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_order_is_inverted_raw_order() {
        // raw is "lower = better"; dense must be "higher = better".
        let lut = DenseRankLut::from_raw_table(&[100u32, 50, 75, 100, 25]);
        assert_eq!(lut.rank(25), 4); // best
        assert_eq!(lut.rank(50), 3);
        assert_eq!(lut.rank(75), 2);
        assert_eq!(lut.rank(100), 1); // worst
        assert_eq!(lut.class_count(), 4);
    }
}
```

**Diff 2 — `crates/pkr-eval/src/lookup_fast.rs`** — integrate into `TableEvaluator`:

```diff
 use super::lookup::{choose, combinadic_rank};
+use super::dense::DenseRankLut;
 use memmap2::Mmap;
 use pkr_contracts::Evaluator;
 use std::fs::File;
 use std::path::Path;
```

```diff
 pub struct TableEvaluator {
     mmap: Mmap,
+    dense: DenseRankLut,
 }

 impl TableEvaluator {
     pub fn new(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
         let file = File::open(path)?;
         let mmap = unsafe { Mmap::map(&file)? };
-        Ok(TableEvaluator { mmap })
+        // Dense LUT built from the same file (raw u32 per 5-card hand).
+        let raws: Vec<u32> = (0..mmap.len() / 4)
+            .map(|i| {
+                let b = &mmap[i * 4..i * 4 + 4];
+                u32::from_le_bytes([b[0], b[1], b[2], b[3]])
+            })
+            .collect();
+        let dense = DenseRankLut::from_raw_table(&raws);
+        Ok(TableEvaluator { mmap, dense })
     }
```

and at the tail of `evaluate_hand` (the 7-card `min` path), convert the winning raw to dense:

```diff
-        best
+        if best == u32::MAX {
+            best
+        } else {
+            self.dense.rank(best)
+        }
     }
 }
```

**Diff 3 — `crates/pkr-eval/src/lib.rs`:**

```diff
 pub mod lookup;
+pub mod dense;
 pub mod slow;
```

**Diff 4 — `crates/pkr-export/src/header.rs`** — bump format version (hash semantics changed in R1+R2):

```diff
 pub const HASH_ALGO_FNV1A64_INFOSET: u8 = pkr_contracts::HASH_ALGO_FNV1A64_INFOSET;
 pub const FORMAT_VERSION_V2: u32 = 2; // version that introduced hash_algo field
+pub const FORMAT_VERSION_V3: u32 = 3; // R-bundle: canonical bucket edges + dense river tiers
```

**Diff 5 — `crates/pkr-export/src/writer.rs`:**

```diff
-use crate::header::{FORMAT_VERSION_V2, HASH_ALGO_FNV1A64_INFOSET};
+use crate::header::{FORMAT_VERSION_V3, HASH_ALGO_FNV1A64_INFOSET};
```

```diff
-        version: FORMAT_VERSION_V2,
+        version: FORMAT_VERSION_V3,
```

**Diff 6 — `crates/pkr-runtime/src/mmap.rs`** — reject pre-bundle blueprints:

```diff
-use pkr_export::header::{FileHeader, FORMAT_VERSION_V2, HASH_ALGO_FNV1A64_INFOSET};
+use pkr_export::header::{FileHeader, FORMAT_VERSION_V3, HASH_ALGO_FNV1A64_INFOSET};
```

```diff
-        if file_header.version < FORMAT_VERSION_V2 {
+        if file_header.version < FORMAT_VERSION_V3 {
             return Err(MmapError::UnsupportedVersion(file_header.version));
         }
```

**Verify.**
1. `cargo test -p pkr-eval` (dense LUT tests + all eval tests).
2. Regenerate artifacts: `SMOKE_FRESH=1 ./smoke.sh` — must pass end-to-end (it trains from scratch).
3. `cargo test --release -p pkr-trainer --test pipeline -- --ignored load_external_blueprint` (smoke.sh runs it; confirms runtime accepts the V3 file).
4. In a 100K proftest, compare vs R-bundle baseline: **infoset count should drop materially** (river collapse ~130K tiers → 117) and `uniform_fallback` in `stats.json` should fall. Exploitability (`expl_mbb`) should improve at equal iterations.

> Memory note: the LUT costs ~16.7 MB (span ≈ 8.34M × u16) per process + one 2.6M-element sort at init (~0.2 s). Acceptable; measured against README's 0.7 s init.


## PART IV — TRAINING PERFORMANCE

Land these one branch at a time, each with `ab_test.sh` A/B. Expected cumulative effect: **+30–60% it/s** plus a 10–30× faster hand evaluation that also collapses precompute time. None of these change infoset semantics (no retrain needed), except where noted.

---

### Task P1-a — Direct 7-card bitboard evaluator `[no hash change; see note]`

**Evidence.** Both evaluators score a 7-card hand by taking the min over **21** 5-card subsets (`slow.rs::NlheEvaluator`, `lookup_fast.rs::TableEvaluator::eval_5_fast`), each subset doing a sort + rank-counting. `evaluate_hand` is called:
- twice per river terminal node (`terminal_payoff`),
- once per river node visit inside `get_infoset_hash` (river tiering),
- 2× per MC sample inside `calculate_ehs` (EHS fallback + all precompute).

A direct bitboard evaluator does the same job in one pass (~40–80 ns vs ~2–3 µs). **Encoding contract:** it must emit the *same raw encoding* as the fixed `eval_5` (R0 applied), so `hand_ranks.bin`-based dense mapping (R2) and the river tier math remain valid, and a differential test can prove equivalence.

**Note on hashes:** the evaluator returns identical values to the R0-fixed one — infoset hashes do not change. (If you skip R0 you are benchmarking a corrupted game; do not.)

**Diff 1 — new file `crates/pkr-eval/src/fast7.rs`** — paste this reference implementation exactly (it is the ONLY version; do not write your own variant):


```rust
use pkr_contracts::Evaluator;

#[inline(always)]
fn straight_high(mask: u16) -> i8 {
    let m = mask as u32;
    if m & 0x100F == 0x100F {
        return 3; // wheel: A,5,4,3,2 -> high rank idx 3 (=5)
    }
    for high in (4..=12).rev() {
        let window = 0x1Fu32 << (high - 4);
        if m & window == window {
            return high as i8;
        }
    }
    -1
}

#[inline(always)]
fn top_n(mask: u16, n: usize) -> [u8; 5] {
    let mut out = [0u8; 5];
    let mut i = 0usize;
    for r in (0..13).rev() {
        if i == n {
            break;
        }
        if mask & (1 << r) != 0 {
            out[i] = r as u8;
            i += 1;
        }
    }
    out
}

/// Counts snapshot shared by the flush/no-flush paths.
struct Counts {
    suit_masks: [u16; 4],
    suit_counts: [u32; 4],
    rank_mask: u16,
    rank_count: [u8; 13],
}

#[inline(always)]
fn classify(c: &Counts) -> u32 {
    // Mirrors eval_5 class order exactly:
    // SF -> quads -> full house -> flush -> straight -> trips -> two pair
    //  -> pair -> high card. Returns !raw (lower = better).
    let mut quad: i8 = -1;
    let mut trips: [i8; 2] = [-1, -1];
    let mut pair1: i8 = -1;
    let mut pair2: i8 = -1;
    for r in (0..13).rev() {
        match c.rank_count[r] {
            4 => quad = r as i8,
            3 => {
                if trips[0] < 0 {
                    trips[0] = r as i8;
                } else if trips[1] < 0 {
                    trips[1] = r as i8;
                }
            }
            2 => {
                if pair1 < 0 {
                    pair1 = r as i8;
                } else if pair2 < 0 {
                    pair2 = r as i8;
                }
            }
            _ => {}
        }
    }

    // Straight flush / flush on the majority suit.
    let flush_mask: Option<u16> = (0..4)
        .find(|&s| c.suit_counts[s] >= 5)
        .map(|s| c.suit_masks[s]);
    if let Some(sm) = flush_mask {
        let sfh = straight_high(sm);
        if sfh >= 0 {
            return !((8u32 << 20) | ((sfh as u32) << 16));
        }
    }

    if quad >= 0 {
        let q = quad as usize;
        let k = top_n(c.rank_mask & !(1 << q), 1)[0] as u32;
        return !((7u32 << 20) | ((q as u32) << 16) | (k << 12));
    }
    if trips[0] >= 0 {
        let pair_rank = trips[1].max(pair1); // second trips also pairs the boat
        if pair_rank >= 0 {
            return !((6u32 << 20)
                | ((trips[0] as u32) << 16)
                | ((pair_rank as u32) << 12));
        }
    }
    if let Some(sm) = flush_mask {
        let fr = top_n(sm, 5);
        return !((5u32 << 20)
            | ((fr[0] as u32) << 16)
            | ((fr[1] as u32) << 12)
            | ((fr[2] as u32) << 8)
            | ((fr[3] as u32) << 4)
            | (fr[4] as u32));
    }
    let sh = straight_high(c.rank_mask);
    if sh >= 0 {
        return !((4u32 << 20) | ((sh as u32) << 16));
    }
    if trips[0] >= 0 {
        let t = trips[0] as usize;
        let kick = top_n(c.rank_mask & !(1 << t), 2);
        return !((3u32 << 20)
            | ((t as u32) << 16)
            | ((kick[0] as u32) << 12)
            | ((kick[1] as u32) << 8));
    }
    if pair2 >= 0 {
        let p1 = pair1 as usize; // highest pair (descending scan)
        let p2 = pair2 as usize; // second pair
        let k = top_n(c.rank_mask & !(1 << p1) & !(1 << p2), 1)[0] as u32;
        return !((2u32 << 20)
            | ((p1 as u32) << 16)
            | ((p2 as u32) << 12)
            | (k << 8));
    }
    if pair1 >= 0 {
        let p = pair1 as usize;
        let kick = top_n(c.rank_mask & !(1 << p), 3);
        return !((1u32 << 20)
            | ((p as u32) << 16)
            | ((kick[0] as u32) << 12)
            | ((kick[1] as u32) << 8)
            | ((kick[2] as u32) << 4));
    }
    let hi = top_n(c.rank_mask, 5);
    !(((hi[0] as u32) << 16)
        | ((hi[1] as u32) << 12)
        | ((hi[2] as u32) << 8)
        | ((hi[3] as u32) << 4)
        | (hi[4] as u32))
}

pub struct FastEvaluator;

impl Evaluator for FastEvaluator {
    #[inline]
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
        let mut c = Counts {
            suit_masks: [0; 4],
            suit_counts: [0; 4],
            rank_mask: 0,
            rank_count: [0; 13],
        };
        let mut seen: u64 = 0;
        let mut n = 0usize;
        for &card in hole.iter().chain(board) {
            if card < 52 && n < 7 && (seen & (1u64 << card)) == 0 {
                seen |= 1u64 << card;
                let s = (card / 13) as usize;
                let r = card % 13;
                c.suit_masks[s] |= 1 << r;
                c.suit_counts[s] += 1;
                c.rank_mask |= 1 << r;
                c.rank_count[r] += 1;
                n += 1;
            }
        }
        if n < 5 {
            return u32::MAX;
        }
        classify(&c)
    }
}
```

**Diff 2 — `crates/pkr-eval/src/lib.rs`:**

```diff
 pub mod lookup;
 pub mod dense;
 pub mod slow;
+pub mod fast7;
```

**Diff 3 — `crates/pkr-eval/src/lookup_fast.rs`** — route `TableEvaluator` through the fast path (keep mmap LUT + dense layer):

```diff
 impl Evaluator for TableEvaluator {
     fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32 {
-        let mut cards = [0u8; 7];
-        let mut total = 0;
-
-        // Filter out sentinel values (≥52) AND duplicate cards
-        for &c in hole.iter().chain(board) {
-            if c < 52 && !cards[..total].contains(&c) {
-                cards[total] = c;
-                total += 1;
-            }
-        }
-
-        if total < 5 {
-            return u32::MAX;
-        }
-
-        cards[..total].sort_unstable_by(|a, b| b.cmp(a)); // sort once, descending
-
-        let mut best = u32::MAX;
-        if total == 5 {
-            ...21-combo min-of-eval_5_fast block...
-        }
-        best
+        let raw = crate::fast7::FastEvaluator.evaluate_hand(hole, board);
+        if raw == u32::MAX {
+            return u32::MAX;
+        }
+        self.dense.rank(raw)
     }
 }
```

(The 21-branch `min` block and `eval_5_fast` can remain in the file — dead code you may delete in the same diff after the differential test passes.)

**Diff 4 — differential test, append to `crates/pkr-eval/src/fast7.rs`:**

```rust
#[cfg(test)]
mod parity_tests {
    use super::*;
    use crate::slow::NlheEvaluator;
    use rand::rngs::StdRng;
    use rand::seq::SliceRandom;
    use rand::SeedableRng;

    #[test]
    fn matches_reference_evaluator_on_random_7_card_hands() {
        let mut rng = StdRng::seed_from_u64(0xC0FFEE);
        let mut deck: Vec<u8> = (0..52).collect();
        let fast = FastEvaluator;
        let slow = NlheEvaluator;
        let mut checked = 0u32;
        for _ in 0..50_000 {
            deck.shuffle(&mut rng);
            for board_len in [3usize, 4, 5] {
                let hole = [deck[0], deck[1]];
                let board = &deck[2..2 + board_len];
                let a = fast.evaluate_hand(&hole, board);
                let b = slow.evaluate_hand(&hole, board);
                assert_eq!(a, b, "hole={hole:?} board={board:?}");
                checked += 1;
            }
        }
        assert!(checked >= 150_000);
    }
}
```

This test is the safety net: it compares the new evaluator against the R0-fixed reference on 150K deals across flop/turn/river sizes. **It must pass before you touch anything else.**

**Verify + A/B.**
1. `cargo test -p pkr-eval --release` — parity + dense tests pass.
2. `./ab_test.sh <baseline-wt> <fasteval-wt> 20 3 8` — expect a solid it/s win (eval was 20–35% of traverse); also re-run `./smoke.sh` and note the turn-table precompute time drop (step 7/8: minutes → seconds).

---

### Task P1-b — Hoist discount math out of the flush loop; compute DCFR in f64 `[mild behavior change]`

**Evidence.** `crates/pkr-cfr/src/table.rs::flush_cpu_batch` calls `update_regret_pfr_plus` once per unique `(idx, action)` group. That function recomputes, **per group**:
- `t.powf(1.5)` twice (α for positive, β=0 for negative regret),
- `1.0 / (t + 1.0).sqrt()`.

`powf` is ~20–50 ns. With ~200–500K unique groups per 256-iteration flush, that is 20–50 ms of pure transcendental-function waste per flush — and the result is identical every time within a batch. Separately, `docs/status.md` correctly notes that in f32, `t^p + 1` saturates past t≈10⁴, silently turning DCFR into vanilla CFR for the rest of a run. Computing the discount in f64 makes the discount real again (still bounded in [0.5, 1), no overflow risk), and hoisting makes it free.

**Diff 1 — `crates/pkr-cfr/src/dcfr.rs`** — add batch-level params (keep existing functions untouched; the Kuhn harness and GPU parity keep using `update_regret_full`):

```rust
/// Precomputed, batch-invariant regret-update parameters (P1-b).
/// Discount math in f64 so it stays exact past t≈10⁴ where f32 saturates
/// (docs/status.md: canonical DCFR degrades to vanilla CFR in f32).
#[derive(Clone, Copy)]
pub struct RegretParams {
    pub w_pos: f32,
    pub w_neg: f32,
    pub gamma: f32,
}

pub fn regret_params_for_iteration(iteration: u32) -> RegretParams {
    let t = iteration as f64;
    if t < TAU as f64 {
        return RegretParams { w_pos: 1.0, w_neg: 1.0, gamma: (1.0 / (t + 1.0).sqrt()) as f32 };
    }
    // Canonical DCFR in f64: bounded in [0.5, 1), never saturates for t < 2^53.
    let pos = t.powf(ALPHA as f64);
    let neg = t.powf(BETA as f64);
    RegretParams {
        w_pos: (pos / (pos + 1.0)) as f32,
        w_neg: (neg / (neg + 1.0)) as f32,
        gamma: (1.0 / (t + 1.0).sqrt()) as f32,
    }
}

/// Apply precomputed params. Must mirror update_regret_full exactly.
#[inline(always)]
pub fn apply_regret_params(current: f32, prev_momentum: f32, delta: f32, p: &RegretParams) -> (f32, f32) {
    let predicted_delta = (1.0 - p.gamma) * prev_momentum + p.gamma * delta;
    let r_pos = current.max(0.0);
    let r_neg = current.min(0.0);
    let new_regret = (p.w_pos * r_pos + p.w_neg * r_neg + predicted_delta).max(0.0);
    (new_regret, predicted_delta)
}

#[cfg(test)]
mod p1b_tests {
    use super::*;

    #[test]
    fn params_match_update_regret_full_in_warmup() {
        let p = regret_params_for_iteration(500);
        let (r1, m1) = update_regret_full(1.0, 0.5, 500, 0.25, DiscountMode::PRODUCTION, MomentumMode::On);
        let (r2, m2) = apply_regret_params(1.0, 0.5, 0.25, &p);
        assert_eq!(r1, r2);
        assert_eq!(m1, m2);
    }

    #[test]
    fn discount_stays_active_past_f32_saturation() {
        // In f32, w(1e6, 1.5) saturates to exactly 1.0. In f64 it must not.
        let p = regret_params_for_iteration(1_000_000);
        assert!(p.w_pos < 1.0, "f64 discount must remain < 1.0, got {}", p.w_pos);
        assert!(p.w_pos > 0.5);
        assert!((p.w_neg - 0.5).abs() < 0.01, "beta=0 gives w_neg ~ 0.5");
    }
}
```

**Diff 2 — `crates/pkr-cfr/src/table.rs::flush_cpu_batch`** — compute once, apply per group:

```diff
 use crate::dcfr::update_regret_pfr_plus;
+use crate::dcfr::{apply_regret_params, regret_params_for_iteration};
```

```diff
         batch.par_sort_unstable_by_key(|item| (item.index, item.action));
         let iteration = batch[0].iteration;
+        let params = regret_params_for_iteration(iteration);
```

```diff
         groups.par_chunks(chunk_size).for_each(|grp_slice| {
             for &(start, end, idx_u32, act_u32) in grp_slice {
-                let mut delta = 0.0f32;
+                let mut delta = 0.0f64; // f64: order-independent group sum
                 for k in start..end {
-                    delta += batch_ref[k].delta;
+                    delta += batch_ref[k].delta as f64;
                 }
                 let idx = idx_u32 as usize;
                 let a = act_u32 as usize;
                 let cur = self.load_rm(idx, a, RM_REGRET) as f32 / SCALE;
                 let mom = self.load_rm(idx, a, RM_MOMENTUM) as f32 / SCALE;
-                let (new_r, new_m) = update_regret_pfr_plus(cur, mom, iteration, delta);
+                let (new_r, new_m) = apply_regret_params(cur, mom, delta as f32, &params);
                 if !new_r.is_finite() || !new_m.is_finite() {
                     warn_nonfinite_regret_once(iteration);
                 }
```

> If `update_regret_pfr_plus` becomes unused, keep it (Kuhn + tests use `update_regret_full`; the wrapper is 6 lines). Note for the `gpu` feature owner: the WGSL shader now differs from the CPU path only in discount precision (f32 vs f64→f32 rounding); mirror by hoisting `w_pos/w_neg/gamma` into a small uniform buffer in the same commit if you care about GPU parity.

**Verify + A/B.** `cargo test -p pkr-cfr` (new param-parity tests + all existing). `ab_test.sh` vs baseline: expect **+2–4%** it/s (more visible at high thread counts where flush share is larger). The f64 discount is a quality change: run the proftest EVAL lines too — late-run exploitability may improve slightly; record it.

---

### Task P1-c — Packed-key sorts + deterministic group sums `[no behavior change]`

**Evidence.** Both flush paths sort with tuple comparators: `batch.par_sort_unstable_by_key(|item| (item.index, item.action))` and the StrategyOp equivalent. Packing `(index, action)` into a single `u64` gives one comparison per probe instead of two and helps the branch predictor. `flush_cpu_batch`'s f64 group sum (P1-b) also removes the last source of parallel-sort nondeterminism in regret deltas.

**Diff — `crates/pkr-cfr/src/table.rs`:**

```diff
     pub fn apply_strategy_batch(&self, ops: &mut Vec<StrategyOp>) -> u64 {
         ops.retain(|op| op.prob != 0.0);
         if ops.is_empty() {
             return 0;
         }
-        ops.par_sort_unstable_by_key(|op| (op.index, op.action));
+        debug_assert!(ops.iter().all(|op| op.action < 8));
+        ops.par_sort_unstable_by_key(|op| ((op.index as u64) << 8) | op.action as u64);
```

```diff
-        batch.par_sort_unstable_by_key(|item| (item.index, item.action));
+        debug_assert!(batch.iter().all(|item| item.action < 8 && item.index < (1 << 55)));
+        batch.par_sort_unstable_by_key(|item| ((item.index as u64) << 8) | item.action as u64);
```

Group detection code compares `index`/`action` fields directly and is untouched — the sort order is identical.

**Verify + A/B.** All tests; `ab_test.sh`: expect **+3–8%** it/s. If the gain is < 2%, keep the change anyway (it also buys determinism) but note it in the ledger.

---

### Task P1-d — Direct-mapped idx cache (kills the 1M-infoset cliff) `[no behavior change]`

**Evidence.** `crates/pkr-cfr/src/table.rs` uses a thread-local `HashMap<u64, usize>` capped at `IDX_CACHE_MAX = 1 << 20`; when full it is `clear()`ed wholesale. With >1M infosets the cache oscillates between empty and full — the README's "throughput degrades past ~10M infosets" cliff actually starts at 1M because of this. A direct-mapped power-of-2 cache has no cliff, one hash multiply on hit, and constant memory.

**Diff — `crates/pkr-cfr/src/table.rs`** — replace the cache block:

```diff
-const IDX_CACHE_INIT: usize = 1 << 18;
-const IDX_CACHE_MAX: usize = 1 << 20;
-
-thread_local! {
-    static IDX_CACHE: RefCell<HashMap<u64, usize, FoldHasher>> =
-        RefCell::new(HashMap::with_capacity_and_hasher(
-            IDX_CACHE_INIT,
-            FoldHasher::default(),
-        ));
-}
-
-#[inline]
-fn cache_lookup(hash: u64) -> Option<usize> {
-    IDX_CACHE.with(|c| c.borrow().get(&hash).copied())
-}
-
-#[inline]
-fn cache_insert(hash: u64, idx: usize) {
-    IDX_CACHE.with(|c| {
-        let mut m = c.borrow_mut();
-        if m.len() >= IDX_CACHE_MAX {
-            m.clear();
-        }
-        m.insert(hash, idx);
-    });
-}
+/// Direct-mapped per-thread idx cache (P1-d). No clear-cliff: colliding
+/// keys evict one slot. Memory: 2^19 * 16 B = 8 MB per thread.
+const CACHE_BITS: u32 = 19;
+const CACHE_SIZE: usize = 1 << CACHE_BITS;
+
+#[derive(Clone, Copy)]
+struct CacheSlot {
+    key: u64,
+    idx: u32,
+}
+
+thread_local! {
+    static IDX_CACHE: RefCell<Vec<CacheSlot>> = RefCell::new(vec![
+        CacheSlot { key: 0, idx: 0 };
+        CACHE_SIZE
+    ]);
+}
+
+#[inline(always)]
+fn cache_slot(hash: u64) -> usize {
+    (hash.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - CACHE_BITS)) as usize
+}
+
+#[inline]
+fn cache_lookup(hash: u64) -> Option<usize> {
+    IDX_CACHE.with(|c| {
+        let slots = c.borrow();
+        let s = &slots[cache_slot(hash)];
+        // FNV-1a outputs are uniform; key==0 collision probability is ~2^-64.
+        if s.key == hash && hash != 0 {
+            Some(s.idx as usize)
+        } else {
+            None
+        }
+    })
+}
+
+#[inline]
+fn cache_insert(hash: u64, idx: usize) {
+    IDX_CACHE.with(|c| {
+        let mut slots = c.borrow_mut();
+        let s = &mut slots[cache_slot(hash)];
+        s.key = hash;
+        s.idx = idx as u32;
+    });
+}
```

and in `load_checkpoint`, replace the cache clear:

```diff
-        IDX_CACHE.with(|c| c.borrow_mut().clear());
+        IDX_CACHE.with(|c| *c.borrow_mut() = vec![CacheSlot { key: 0, idx: 0 }; CACHE_SIZE]);
```

> `HashMap`/`FoldHasher` imports may become unused — remove `use std::collections::HashMap;`/`use foldhash::fast::RandomState as FoldHasher;` if `cargo clippy` flags them (papaya still uses `FoldHasher` — check before deleting).

**Tuning knob:** `CACHE_BITS = 19` (8 MB/thread) targets ≤500K active infosets per thread window. If you train >5M infosets and see low hit rates in `metrics.csv` (`cache_hit_rate`), bump to 20 (16 MB/thread).

**Verify + A/B.** All tests; `ab_test.sh` at 100K iters (small gain expected, cache was warm under the cliff) **and** a 1M-iteration run comparing `cache_hit_rate` + it/s — this is where the cliff shows: baseline hit rate decays as the HashMap clears; the direct-mapped cache should hold a stable hit rate and win clearly. Record both.

---

### Task P1-e — O(1) `history_signature` `[no behavior change — bit-identical]`

**Evidence.** `state.rs::history_signature` rescans `history[0..history_len]` on every node visit to count raises — O(depth) work, ~30–60 ns/node, ~8M nodes/s. The same value is trivially maintainable incrementally.

**Diff — `crates/pkr-core/src/state.rs`:**

```diff
 pub struct GameState {
@@
     pub actions_this_street: u8,
     pub raises_this_street: u8,
+    pub raises_total: u8, // P1-e: all Bet actions over the whole hand
     pub abstract_history: [u8; 32],
```

```diff
 pub struct UndoRecord {
@@
     actions_this_street: u8,
     raises_this_street: u8,
+    raises_total: u8,
     history_len: usize,
```

In `GameState::new`, add `raises_total: 0,` to both the state literal and the `UndoRecord` template inside `undo_stack`. In `push_undo`, add `raises_total: self.raises_total,`.

In `apply_action_internal`, the `Bet` arm:

```diff
                 self.street_bets[actor] = total;
                 self.raises_this_street = self.raises_this_street.saturating_add(1);
+                self.raises_total = self.raises_total.saturating_add(1);
```

In `undo_action`:

```diff
         self.actions_this_street = rec.actions_this_street;
         self.raises_this_street = rec.raises_this_street;
+        self.raises_total = rec.raises_total;
```

And the signature itself:

```diff
     pub fn history_signature(&self) -> u32 {
-        let mut raises: u8 = 0;
-        for i in 0..self.history_len as usize {
-            if matches!(self.history[i].kind, ActionKind::Bet(_)) {
-                raises = raises.saturating_add(1);
-            }
-        }
         let last_was_bet = if self.history_len > 0 {
             matches!(
                 self.history[self.history_len as usize - 1].kind,
                 ActionKind::Bet(_)
             )
         } else {
             false
         };
         (self.actions_this_street as u32 & 0xFF)
-            | ((raises as u32 & 0xFF) << 8)
+            | ((self.raises_total as u32 & 0xFF) << 8)
             | ((last_was_bet as u32) << 16)
     }
```

**Semantics guard:** `raises_total` counts every `Bet` recorded in `history` — exactly what the old scan computed (the recording happens in the same function that appends history), so hashes are **bit-identical**. Add this test to `state.rs`'s test module (or create one):

```rust
#[cfg(test)]
mod p1e_tests {
    use super::*;

    #[test]
    fn signature_matches_naive_scan() {
        let mut s = GameState::new(200.0, 1.0, 2.0);
        let acts = [
            Action { player: 0, kind: ActionKind::Call },
            Action { player: 1, kind: ActionKind::Check },
            Action { player: 0, kind: ActionKind::Bet(3.0) },
            Action { player: 1, kind: ActionKind::Bet(9.0) },
        ];
        let mut naive_raises = 0u32;
        for a in &acts {
            if matches!(a.kind, ActionKind::Bet(_)) {
                naive_raises += 1;
            }
            s.apply_action_in_place(a);
            let sig = s.history_signature();
            let raises_field = (sig >> 8) & 0xFF;
            assert_eq!(raises_field, naive_raises);
        }
        s.undo_action();
        let sig = s.history_signature();
        assert_eq!((sig >> 8) & 0xFF, 1, "undo must restore raises_total");
    }
}
```

**Verify + A/B.** All tests; `ab_test.sh`: expect **+2–4%**.

---

### Task P1-f — Remove the 480-byte per-node memset `[no behavior change]`

**Evidence.** `traverse` zeroes `let mut action_indices = [[0usize; 10]; K];` (6 × 10 × 8 B = 480 B) on **every node visit** — at ~8.1M nodes/s that is several GB/s of pure memset. Only entries `< action_counts[a]` are ever read. Restructure to an 8-entry tag array written exactly once per concrete action.

**Diff — `crates/pkr-cfr/src/traversal.rs`, inside `traverse`:**

```diff
-    let mut action_counts = [0usize; K];
-    let mut action_indices = [[0usize; 10]; K];
-    for (idx, action) in num_actions.iter().enumerate() {
-        let a = abstract_action_index(&action.kind, current);
-        if action_counts[a] < 10 {
-            action_indices[a][action_counts[a]] = idx;
-            action_counts[a] += 1;
-        }
-    }
+    let mut action_counts = [0usize; K];
+    let mut bucket_of_action = [0u8; 8]; // index = concrete action idx
+    for (idx, action) in num_actions.iter().enumerate() {
+        let a = abstract_action_index(&action.kind, current);
+        bucket_of_action[idx] = a as u8;
+        action_counts[a] += 1;
+    }
```

Then replace every `action_indices[a][ordinal]` lookup with a rescan helper. The traverser branch:

```diff
             let pick_idx = action_indices[a][rng.random_range(0..count)];
+            let ordinal = rng.random_range(0..count);
+            let mut pick_idx = 0usize;
+            let mut seen = 0usize;
+            for (i, &b) in bucket_of_action.iter().enumerate().take(num_actions_n) {
+                if b as usize == a {
+                    if seen == ordinal {
+                        pick_idx = i;
+                        break;
+                    }
+                    seen += 1;
+                }
+            }
```

The opponent branch:

```diff
-        let pick_idx = action_indices[sampled_abstract][rng.random_range(0..count)];
+        let ordinal = rng.random_range(0..count);
+        let mut pick_idx = 0usize;
+        let mut seen = 0usize;
+        for (i, &b) in bucket_of_action.iter().enumerate().take(num_actions_n) {
+            if b as usize == sampled_abstract {
+                if seen == ordinal {
+                    pick_idx = i;
+                    break;
+                }
+                seen += 1;
+            }
+        }
```

To avoid duplicating that block, extract a local closure at the top of `traverse` (after `bucket_of_action` is filled):

```rust
    let pick_in_bucket = |bucket: usize, ordinal: usize| -> usize {
        let mut seen = 0usize;
        for (i, &b) in bucket_of_action.iter().enumerate().take(num_actions_n) {
            if b as usize == bucket {
                if seen == ordinal {
                    return i;
                }
                seen += 1;
            }
        }
        unreachable!("bucket {bucket} ordinal {ordinal} out of range");
    };
```

and use `let pick_idx = pick_in_bucket(a, rng.random_range(0..count));` at both sites. (Keep `action_counts` — masking logic depends on it.)

**Verify + A/B.** All tests (traversal tests exercise both branches); `ab_test.sh`: expect **+2–5%**.

---

### Task P2-a — 10–50× faster checkpoints `[no behavior change]`

**Evidence.** `CompactRegretTable::save_checkpoint` writes `n × 12` atomic loads + 4-byte `write_all` calls, then `n × 6` u64s the same way. At 5M infosets that is ~500 MB through a per-element loop. `AtomicI32`/`AtomicU64` are guaranteed by `std` to have the same size/alignment/valid-bit-patterns as `i32`/`u64`, so the arrays can be streamed as flat slices.

**Diff — `crates/pkr-cfr/src/table.rs`:**

```diff
-        let rm_entries = n * RM_STRIDE;
-        for i in 0..rm_entries {
-            w.write_all(&self.data[i].load(Ordering::Relaxed).to_le_bytes())?;
-        }
-        let sum_entries = n * SUM_STRIDE;
-        for i in 0..sum_entries {
-            w.write_all(&self.strategy_sum[i].load(Ordering::Relaxed).to_le_bytes())?;
-        }
+        // P2-a: stream the backing arrays directly. SAFETY: AtomicI32 has the
+        // same size/alignment as i32 and all bit patterns are valid (std
+        // guarantee); we only take a shared view and elements are never
+        // mutated through it.
+        let rm_entries = n * RM_STRIDE;
+        let rm: &[i32] = unsafe {
+            std::slice::from_raw_parts(self.data.as_ptr() as *const i32, rm_entries)
+        };
+        w.write_all(unsafe { std::slice::from_raw_parts(rm.as_ptr() as *const u8, rm_entries * 4) })?;
+        let sum_entries = n * SUM_STRIDE;
+        let sums: &[u64] = unsafe {
+            std::slice::from_raw_parts(self.strategy_sum.as_ptr() as *const u64, sum_entries)
+        };
+        w.write_all(unsafe { std::slice::from_raw_parts(sums.as_ptr() as *const u8, sum_entries * 8) })?;
```

and the load side:

```diff
-        let rm_entries = n * RM_STRIDE;
-        for i in 0..rm_entries {
-            let v = i32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
-            self.data[i].store(v, Ordering::Relaxed);
-        }
-        let sum_entries = n * SUM_STRIDE;
-        for i in 0..sum_entries {
-            let v = u64::from_le_bytes(read(&mut p, 8)?.try_into().unwrap());
-            self.strategy_sum[i].store(v, Ordering::Relaxed);
-        }
+        let rm_entries = n * RM_STRIDE;
+        let rm_bytes = rm_entries * 4;
+        let rm_bytes_slice = read(&mut p, rm_bytes)?;
+        // SAFETY: exclusive mutable view for initialization; same layout
+        // guarantee as the save side.
+        let rm_dst: &mut [i32] = unsafe {
+            std::slice::from_raw_parts_mut(self.data.as_mut_ptr() as *mut i32, rm_entries)
+        };
+        rm_dst.copy_from_slice(unsafe {
+            std::slice::from_raw_parts(rm_bytes_slice.as_ptr() as *const i32, rm_entries)
+        });
+        let sum_entries = n * SUM_STRIDE;
+        let sum_bytes = sum_entries * 8;
+        let sum_bytes_slice = read(&mut p, sum_bytes)?;
+        let sum_dst: &mut [u64] = unsafe {
+            std::slice::from_raw_parts_mut(self.strategy_sum.as_mut_ptr() as *mut u64, sum_entries)
+        };
+        sum_dst.copy_from_slice(unsafe {
+            std::slice::from_raw_parts(sum_bytes_slice.as_ptr() as *const u64, sum_entries)
+        });
```

> Both sides assume little-endian (all supported platforms; the rest of the format is LE too). If you need endianness safety, add a debug assertion on `cfg(target_endian = "little")`.

**Verify.** Round-trip test: `save_checkpoint` → mutate nothing → `load_checkpoint` into a fresh `with_capacity` table → `snapshot()` fields equal. `smoke.sh` (which checkpoints twice in 10 iterations) passes. Time a 5M-infoset checkpoint before/after and record.

---

### Task P2-b — Turn-table precompute feasibility (uses P1-a) `[measurement + knob]`

**Evidence.** `generate_turn_table` enumerates C(52,6) × 15 = 305,377,800 entries; each entry runs `calculate_ehs` with `EHS_SAMPLES` MC draws (2 × 7-card evals each). With the old 21-combo evaluator and `EHS_SAMPLES=10` (run.sh) this is ~6.1e9 evaluations ≈ many hours/days. `smoke.sh` dodges it with `EHS_SAMPLES=1`; the *real* pipeline in `run.sh` step 7/8 is effectively infeasible as written. P1-a's evaluator alone cuts per-eval cost ~30–50×; the MC sample count remains the second lever.

**Diff — `crates/pkr-abstraction/src/bin/precompute.rs`** — add a progress + ETA line so the agent can prove feasibility (inside `generate_turn_table`, before the `par_chunks_mut`):

```rust
    let t0 = std::time::Instant::now();
    let total_entries = entries as u64;
```

and after the parallel loop:

```rust
    let secs = t0.elapsed().as_secs_f64();
    println!(
        "turn table: {} entries in {:.1}s ({:.0} entries/s, EHS_SAMPLES={})",
        total_entries,
        secs,
        total_entries as f64 / secs.max(1e-9),
        std::env::var("EHS_SAMPLES").unwrap_or_else(|_| "1000".into())
    );
```

**Protocol.**
1. After P1-a, time `EHS_SAMPLES=10 ... turn` on a 10-minute bounded run, extrapolate: `(entries_done / elapsed) × total_entries`.
2. Decision rule: full table ≤ 4 h wall → run it and cache the artifact. Otherwise drop `EHS_SAMPLES` to 5 for the turn (bucket **quality** impact is second-order because k-means quantizes to ≤255 centroids anyway) and record the choice in the manifest.
3. Record numbers in `docs/baseline.md`. This task is "done" when `run.sh` step 7 completes in bounded, documented time.

> Structural fix (Q5 in Part VI) replaces the C(52,6)×15 enumeration with a two-level texture abstraction; do not attempt it before the R-bundle and P1-a land.

## PART V — RUNTIME LOOKUP (VPS side)

Baseline: `SolverHandle::get_advice_fast` = branchy binary search with `from_le_bytes` + bounds checks per probe, over mmap'd sorted keys. p99 < 1 ms is already fine, but these upgrades shrink p99 and cache-line traffic 2–5×, which matters on cheap VPS CPUs.

---

### Task P3-a — Branchless binary search over cast keys `[no format change]`

**Diff — `crates/pkr-runtime/src/lookup.rs`** — replace the body of `get_advice_fast` with this exact version:

```rust
    pub fn get_advice_fast(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        let keys: &[u64] = bytemuck::try_cast_slice(self.mmap.keys_data())
            .expect("blueprint key section must be 8-byte aligned");
        let num_keys = keys.len();
        if num_keys == 0 {
            return None;
        }
        let mut base = 0usize;
        let mut size = num_keys;
        while size > 1 {
            let half = size / 2;
            let mid = base + half;
            base = if keys[mid] <= infoset_hash { mid } else { base };
            size -= half;
        }
        if keys[base] != infoset_hash {
            return None;
        }
        let max_actions = self.mmap.file_header().max_actions_k as usize;
        if max_actions > 16 {
            return None;
        }
        let cdf_start = base * max_actions;
        let cdf_end = cdf_start + max_actions;
        let cdf = self.mmap.cdf_data();
        if cdf_end > cdf.len() {
            return None;
        }
        let mut prob = [0u8; 16];
        prob[..max_actions].copy_from_slice(&cdf[cdf_start..cdf_end]);
        Some(SotaAdvice {
            cdf_probabilities: prob,
            len: max_actions as u8,
        })
    }
```

**Add lookup tests** (append to `lookup.rs`):

```rust
#[cfg(test)]
mod p3a_tests {
    use super::*;
    use crate::mmap::MmapReader;
    use pkr_export::writer::write_blueprint;
    use pkr_cfr::table::CompactRegretTable;
    use tempfile::tempdir;

    fn build_tiny_blueprint(dir: &std::path::Path) -> std::path::PathBuf {
        let table = CompactRegretTable::with_capacity(1024);
        let h1 = 0x1000u64;
        let h2 = 0x8000u64;
        table.add_strategy_sum(h1, 0, 0.8);
        table.add_strategy_sum(h1, 1, 0.2);
        table.add_strategy_sum(h2, 3, 1.0);
        let keys = table.get_keys();
        let path = dir.join("bp.bin");
        write_blueprint(path.to_str().unwrap(), &table, &keys);
        path
    }

    #[test]
    fn search_finds_present_and_rejects_absent() {
        let dir = tempdir();
        let path = build_tiny_blueprint(dir.path());
        let handle = SolverHandle::new(MmapReader::new(&path).unwrap());
        assert!(handle.get_advice_fast(0x1000).is_some());
        assert!(handle.get_advice_fast(0x8000).is_some());
        assert!(handle.get_advice_fast(0x0FFF).is_none());
        assert!(handle.get_advice_fast(0x1001).is_none());
        assert!(handle.get_advice_fast(u64::MAX).is_none());
        assert!(handle.get_advice_fast(0).is_none());
    }
}
```

**Verify.** `cargo test -p pkr-runtime --release` (tempfile is already a dev-dependency; `pkr-cfr`/`pkr-export` must be added to `[dev-dependencies]` of `pkr-runtime` for the test — check `crates/pkr-runtime/Cargo.toml` and add `pkr-cfr = { workspace = true }` + `pkr-export = { workspace = true }` under `[dev-dependencies]` if missing; `pkr-export` is already a regular dep). Then micro-benchmark before/after with `criterion` (T1-b): expect 2–3× median latency drop at 500K keys.

---

### Task P3-b — Wire the already-built FMph for O(1) lookup `[format addition, backward-compatible]`

**Evidence.** `pkr-export::fmph::build_fmph` constructs a minimal perfect hash, but the writer never serializes it and the runtime never evaluates it — dead infrastructure called out in `docs/status.md`. MPHF gives O(1) key→index with 2 multiplies + 1 load of the displacement table, plus one key read for verification.

**Design.** Append an *optional* FMph section after the CDF block. Layout: `FmphHeader (40 B) | displacements: u32 × bucket_count`. Readers of version 3 without the section (section flag = 0 in `level_count`) fall back to binary search — full backward compatibility.

**Diff 1 — `crates/pkr-export/src/writer.rs`:**

```diff
 use crate::header::{FORMAT_VERSION_V3, HASH_ALGO_FNV1A64_INFOSET};
+use crate::fmph::build_fmph;
 use crate::translate::compute_translation;
 use pkr_cfr::table::CompactRegretTable;
@@
     file.write_all(&(num_keys as u32).to_le_bytes()).unwrap();
     file.write_all(&((K * num_keys) as u32).to_le_bytes()).unwrap();
     file.write_all(&key_bytes).unwrap();
     file.write_all(&cdf_bytes).unwrap();
+
+    // Optional FMph section (P3-b). Build can fail (rare MPHF retries) —
+    // fall back to binary-search-only file by simply not writing it.
+    let fmph_result = std::panic::catch_unwind(|| build_fmph(keys));
+    if let Ok(fmph) = fmph_result {
+        let mut hdr = fmph.to_header();
+        hdr.level_count = 1; // section-present flag (level_count >= 1)
+        file.write_all(bytemuck::bytes_of(&hdr)).unwrap();
+        for d in &fmph.displacements {
+            file.write_all(&d.to_le_bytes()).unwrap();
+        }
+    }

     file.flush().unwrap();
 }
```

**Diff 2 — `crates/pkr-runtime/src/mmap.rs`** — parse the optional tail:

```diff
 pub struct MmapReader {
     mmap: Mmap,
     file_header: FileHeader,
     offset_keys: usize,
     num_keys: usize,
     offset_cdf: usize,
     len_cdf: usize,
+    fmph: Option<FmphView>,
 }
+
+/// Parsed FMph section (P3-b).
+pub struct FmphView {
+    pub seed1: u64,
+    pub seed2: u64,
+    pub bucket_count: usize,
+    pub num_keys: usize,
+    pub displacements: Vec<u32>,
+}
```

In `MmapReader::new`, after the existing length check, attempt to parse a trailing section:

```rust
        // Optional FMph tail (P3-b): FmphHeader + u32 displacements.
        let fmph = {
            let tail_off = offset_cdf + cdf_bytes_len;
            let hdr_len = std::mem::size_of::<pkr_export::header::FmphHeader>();
            if mmap.len() >= tail_off + hdr_len {
                let hdr: pkr_export::header::FmphHeader =
                    *bytemuck::from_bytes(&mmap[tail_off..tail_off + hdr_len]);
                // Sanity: header must describe exactly the key count and
                // carry the section-present flag (level_count == 1).
                if hdr.level_count == 1
                    && hdr.num_keys as usize == key_count
                    && mmap.len() >= tail_off + hdr_len + hdr.max_level_size as usize * 4
                {
                    let disp_off = tail_off + hdr_len;
                    let mut displacements = Vec::with_capacity(hdr.max_level_size as usize);
                    for i in 0..hdr.max_level_size as usize {
                        let b = &mmap[disp_off + i * 4..disp_off + i * 4 + 4];
                        displacements.push(u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                    }
                    Some(FmphView {
                        seed1: hdr.seed1,
                        seed2: hdr.seed2,
                        bucket_count: hdr.max_level_size as usize,
                        num_keys: key_count,
                        displacements,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        };
```

and add `fmph` to the returned struct.

**Diff 3 — `crates/pkr-runtime/src/lookup.rs`** — O(1) probe path:

```diff
 impl SolverHandle {
     pub fn get_advice_fast(&self, infoset_hash: u64) -> Option<SotaAdvice> {
+        if let Some(f) = &self.mmap.fmph {
+            let h = |key: u64, seed: u64| key.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(seed);
+            let b = (h(infoset_hash, f.seed1) as usize) % f.bucket_count;
+            let d = f.displacements[b] as u64;
+            let idx = ((h(infoset_hash, f.seed2).wrapping_add(d) as usize) % f.num_keys) as usize;
+            // MPHF hit => keys[idx] == hash. One verification read.
+            let keys: &[u64] = bytemuck::try_cast_slice(self.mmap.keys_data())
+                .expect("keys aligned");
+            if keys[idx] == infoset_hash {
+                let max_actions = self.mmap.file_header().max_actions_k as usize;
+                let cdf = self.mmap.cdf_data();
+                let cdf_end = (idx + 1) * max_actions;
+                if max_actions <= 16 && cdf_end <= cdf.len() {
+                    let mut prob = [0u8; 16];
+                    prob[..max_actions].copy_from_slice(&cdf[idx * max_actions..cdf_end]);
+                    return Some(SotaAdvice { cdf_probabilities: prob, len: max_actions as u8 });
+                }
+            }
+            // Fall through to binary search on MPHF miss (foreign hash).
+        }
         let keys: &[u64] = bytemuck::try_cast_slice(self.mmap.keys_data())
             .expect("blueprint key section must be 8-byte aligned");
```

(This reuses P3-a's search as the fallback path — apply P3-a first.)

> The `hash_key` multiplier here must match `pkr-export/src/fmph.rs::hash_key` (`0x9E3779B97F4A7C15` + seed) — it does, both are the golden-ratio constant. `build_fmph` is randomized (500K attempt loop); if it fails to converge it panics — the writer's `catch_unwind` degrades gracefully to search-only.

**Verify.** Extend the P3-a round-trip test: assert `handle.mmap.fmph.is_some()` on a fresh export and that `get_advice_fast` answers both present keys. Benchmark: criterion lookup bench (T1-b) at 500K keys — expect median latency to drop below the branchless-search numbers and, more importantly, flat cache-line cost as the table grows.


## PART VI — BLUEPRINT-STRENGTH EXPERIMENTS (each = one A/B, keep the winners)

These change infoset semantics or training math. Land them **after** the R-bundle so the baseline is honest. Each one: branch → one change → proftest with `--eval-every 25000 --eval-deals 20000` → compare `expl_mbb` trajectory + `stats.json` → keep or revert. **Never land two Q-tasks in one branch.**

---

### Q1 — River tier sweep `[HASH]` (one line, high leverage)

Now that R2 makes `hand_rank` dense (1..7462), the tier count is `7462 >> shift`:

| `>> shift` | tiers | river infosets (with 200 board buckets) | trade-off |
|---|---|---|---|
| 6 (current) | ~117 | ~23K | fastest convergence, coarsest |
| 5 | ~233 | ~47K | likely sweet spot |
| 4 | ~466 | ~93K | needs more iterations |
| 3 | ~933 | ~187K | only for 10M+ iter runs |

**Diff — `crates/pkr-abstraction/src/lib.rs`, river arm of `get_infoset_hash`:**

```diff
+        // Q1 knob: dense hand rank -> tier. 6 => 117 tiers.
+        const RIVER_TIER_SHIFT: u32 = 5;
                 let hand_rank = self.evaluator.evaluate_hand(hole, board) as u64;
-                let hand_bucket = hand_rank >> 6;
+                let hand_bucket = hand_rank >> RIVER_TIER_SHIFT;
```

**A/B.** 100K iters: expect small gains; 1M iters: `>> 5` should beat `>> 6` on `expl_mbb` while it/s drops slightly (more infosets → more papaya traffic). Watch `max|r|` and `uniform_fallback`.

### Q2 — Strategy-sum recency discount `[HASH, checkpoint v5]` (advanced)

**Problem.** The average strategy is a plain sum over all iterations (the γ=2 DCFR discount exists as dead code). Late-iteration strategies are drowned by early noise; DCFR's paper discounts strategy sums for exactly this reason.

**Design (lazy decay).** Add a per-infoset `last_touch: Vec<AtomicU32>` and a constant `S_DECAY: f64 = 0.999` (≈ half-life 693 iterations; also try 0.9999). When a strategy op touches `(idx, action)` during `apply_strategy_batch` (cells are group-exclusive, so this is race-free), apply `s ← s × S_DECAY^(t_now − last_touch)` before adding. Store decay factor per **cell** or per **infoset** (per infoset is simpler: one extra `Vec<AtomicU32>` of length `capacity`, 4 B/infoset = 20 MB at 5M).

**Diff sketch — `crates/pkr-cfr/src/table.rs`:**

```rust
    // field on CompactRegretTable
    last_touch: Vec<AtomicU32>,
    // init: vec![AtomicU32::new(0); capacity]

    #[inline(always)]
    fn decay_and_touch(&self, idx: usize, now: u32, decay: f64) {
        let cell = &self.last_touch[idx];
        let prev = cell.load(Ordering::Relaxed) as u64;
        let delta = (now as u64).saturating_sub(prev);
        if delta > 0 && prev > 0 {
            // decay every strategy_sum cell of this infoset
            let factor = decay.powi(delta.min(10_000) as i32);
            for a in 0..SUM_STRIDE {
                let bits_cell = &self.strategy_sum[idx * SUM_STRIDE + a];
                let mut cur_bits = bits_cell.load(Ordering::Relaxed);
                loop {
                    let cur = f64::from_bits(cur_bits);
                    let new_bits = (cur * factor).to_bits();
                    match bits_cell.compare_exchange_weak(
                        cur_bits, new_bits, Ordering::Relaxed, Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(actual) => cur_bits = actual,
                    }
                }
            }
        }
        cell.store(now, Ordering::Relaxed);
    }
```

Call `decay_and_touch(idx, batch_iteration, S_DECAY)` at the top of each group in `apply_strategy_batch`. Bump the checkpoint magic to `PKRCKPT5` (persist `last_touch`; version-gate the loader like the R-bundle did). **A/B** at 100K and 1M iters; expect visibly lower `expl_mbb` at 1M. If the win is marginal at 100K, prefer the bigger budget.

### Q3 — Sizing ladder `[HASH]`

**Diff — `crates/pkr-core/src/state.rs`** — hoist the ladder to a constant (currently hard-coded in `legal_actions_into` twice):

```rust
/// Concrete bet sizes as fractions of the pot (Q3). Buckets 2/3/4 map to
/// these 1:1 via abstract_action_index's 0.75/1.5 edges.
pub const BET_FRACTION_LADDER: [f32; 3] = [0.5, 1.0, 2.0];
```

Replace both `for &frac in &[0.5, 1.0, 2.0]` loops with `for &frac in &BET_FRACTION_LADDER`.

**A/B candidates** (one per branch): `[0.33, 0.66, 1.33]` (geometric — matches modern GTO sizings better than 0.5/1/2), `[0.4, 1.0, 2.5]`, and a 4-size variant `[0.33, 0.66, 1.33, 2.5]` (legal: fold/call + 4 raises + all-in = 7 ≤ 8 slots). Note: the 4-size variant changes `abstract_action_index` edges too (0.5/1.0/2.0 boundaries) — keep bucket edges consistent with the ladder in the same branch.

### Q4 — Deeper raise cap `[HASH]`

`MAX_RAISES_PER_STREET = 3` in `legal_actions_into`. Try 4: doubles some tree branches (it/s ↓ maybe 15–25%) in exchange for correct play in 4-bet pots. A/B at equal iteration counts; decide by `expl_mbb` per second of wall time, not per iteration.

### Q5 — Turn abstraction restructure `[HASH]` (design sketch, L)

The C(52,6)×15 enumeration is the last structurally expensive precompute. A two-level scheme: (1) k-means the C(52,4) = 270,725 turn **boards** on the flop-style 10-dim EHS histogram (already implemented for flops in `generate_flop_buckets` — reuse verbatim); (2) infoset feature = `(board_bucket, hand EHS decile)` computed at runtime: EHS at turn still needs MC (~50 evals × 60 ns ≈ 3 µs after P1-a — acceptable at ~30 turn nodes/iter ≈ +90 µs/iter ≈ −20% it/s) — or precompute the 270,725 × 1326 dense-EHS matrix once (1.4 GB — mmap it). Prototype behind a feature flag; A/B vs the current turn table.

### Q6 — Sampled-BR consistency (variance, no retrain)

The current `sampled_exploitability` walks each deal independently: the BR picks the best action **per concrete deal**, but two deals in the same bucket must play the same strategy — so the metric overestimates exploitability (hindsight bias). It is fine for *relative* A/B, but document it, and if you need absolute numbers: group sampled deals by infoset hash (two-pass: pass 1 collects per-bucket EVs, pass 2 evaluates the argmax per bucket). Track as an issue; do not block the perf work on it.

### Q7 — Variance-reduced CFR variants (research, keep behind flags)

The traversal is external-sampling MCCFR. Candidates worth a Kuhn-harness A/B before touching NLHE: (a) outcome sampling with per-action probability weighting (lower variance per iteration, worse it/s — measure); (b) plain vs PCFR+ momentum at different γ schedules (the Kuhn harness says momentum-off converges 3× faster on Kuhn — verify on NLHE at 1M iters before changing the default); (c) warm-starting from a checkpoint with fresh discount epoch. Use `kuhn_experiment` configs as the template. Only promote a variant to production after it wins on **both** Kuhn exploitability and NLHE `expl_mbb`.

### Q8 — Subgame river re-solve (research, L)

`pkr-cfr::riversolve` is a stub (regrets reset each iteration; O(1700²) re-enumeration per iteration — effectively dead, as `docs/status.md` admits). If pursued: fix it into a real depth-limited CFR+ over the river subgame with ranges from card removal (enumeration is only C(45,2)=990 villain combos × 44 rivers with memoized showdown results — build a 990×44 win matrix once per board, ~43K evals ≈ 3 ms with P1-a). Wire via `pkr-exploit` at query time. This is the largest single strength upgrade available but is a project of its own; keep it last.

---

## PART VII — MASTER EXECUTION ORDER & ACCEPTANCE GATES

```
Stage 0  T0-a seed ─ T0-b ab_test.sh ─ T0-c proftest EVAL + native ─ T0-d baseline sheet
         gate: A-vs-A it/s delta ≤ ±2%; EVAL lines present; baseline recorded
Stage 1  R-bundle (R0 + R1 + R2, one branch, one retrain)
         gate: all tests pass; SMOKE_FRESH smoke green; V3 blueprint loads
         A/B: expl_mbb at 100K must improve vs baseline; infoset count drops
Stage 2  P1-a fast eval → P1-b → P1-c → P1-d → P1-e → P1-f   (each: diff, tests, ab_test.sh)
         gate: each perf task ≥ +2% it/s OR documented determinism benefit;
         cumulative target ≥ +25% it/s vs Stage-0 baseline; eval parity test green
Stage 3  P2-a checkpoints ─ P2-b turn feasibility ─ P3-a branchless search ─ P3-b FMph
         gate: checkpoint round-trip test; run.sh step 7 completes bounded;
         lookup bench median improves; blueprint loads on V3 with/without FMph
Stage 4  Q1 river tiers ─ Q3 ladder ─ Q4 raise cap ─ Q2 decay (advanced) ─ Q5+ (research)
         gate: each Q keeps/expands expl_mbb win at 1M iterations or is reverted
```

**Regression gates to wire once (T1, one small PR):**
1. **Kuhn CI gate** — in `kuhn_experiment.rs::main`, after the verdict block:

```diff
     match best {
         Some((name, val)) => println!(
             "best_mode={} best_exploitability={:.3e}",
             name, val
         ),
         None => println!("no finite mode."),
     }
+
+    // T1: CI gate — fail the build when Kuhn exploitability regresses.
+    if let Ok(gate) = std::env::var("PKR_EXPL_GATE") {
+        let gate: f32 = gate.parse().expect("PKR_EXPL_GATE must be a float");
+        if let Some((name, val)) = best {
+            if val > gate {
+                eprintln!("GATE FAIL: {} expl {:.3e} > gate {:.3e}", name, val, gate);
+                std::process::exit(1);
+            }
+        } else {
+            std::process::exit(1);
+        }
+    }
```

   then `PKR_EXPL_GATE=5e-3 cargo run --release -p pkr-testgames --bin kuhn-experiment` in `fast.sh`/CI.
2. **Table-size validation** — in `binaries/pkr-trainer/src/main.rs`, after abstraction setup:

```rust
    // T1: fail fast on missing/undersized abstraction tables. The MC-EHS
    // fallback is ~100x slower; better to die loudly (see warn_mc_fallback_once).
    fn expect_size(path: &std::path::Path, want: usize, what: &str) {
        let got = std::fs::metadata(path)
            .unwrap_or_else(|e| panic!("{what}: cannot stat {}: {e}", path.display()))
            .len() as usize;
        assert_eq!(got, want, "{what}: expected {want} bytes, got {got}");
    }
    if let Some(p) = &cli.preflop_table {
        expect_size(p, 1326, "preflop table");                    // C(52,2)
    }
    if let Some(p) = &cli.flop_table {
        expect_size(p, 25_989_600, "flop table");                 // C(52,5) * 10
    }
    if let Some(p) = &cli.turn_table {
        expect_size(p, 305_377_800, "turn table");                // C(52,6) * 15
    }
    if let Some(p) = &cli.river_table {
        expect_size(p, 2_598_960, "river table");                 // C(52,5)
    }
    if let Some(p) = &cli.flop_buckets {
        expect_size(p, 22_100, "flop buckets");                   // C(52,3)
    }
```

3. **Criterion micro-benches** — add `criterion = "0.5"` to `[dev-dependencies]` of `pkr-eval` and `pkr-cfr` plus `[[bench]] name = "kernels" harness = false` to both `Cargo.toml`s; then `crates/pkr-eval/benches/kernels.rs` benchmarking `FastEvaluator` on 1K random deals and `calculate_ehs`; and `crates/pkr-cfr/benches/kernels.rs` benchmarking `cache_lookup/insert`, `flush_cpu_batch` on 500K synthetic `BatchItem`s, and `apply_strategy_batch`. Run `cargo bench` before/after each P-task; the numbers explain *why* the end-to-end A/B moves.
4. **A/B report generator** — a 20-line python script that ingests two `metrics.csv` files and prints median it/s, p95 flush share, `cache_hit_rate`, and final `expl_mbb`; commit it next to `ab_test.sh`.

---

## APPENDIX A — INVARIANT CHEAT SHEET (what must always stay true)

| Invariant | Guarded by |
|---|---|
| `eval_5`/`FastEvaluator` raw encodings identical | fast7 parity test (P1-a Diff 4) |
| Dense rank is monotone in raw rank | `dense_order_is_inverted_raw_order` (R2) |
| Bucket mapping identical in traversal / history / exploit / translation | `r1_bucket_mapping_is_canonical` + anchors test (R1) |
| `history_signature` == naive scan | `signature_matches_naive_scan` (P1-e) |
| Regret params == `update_regret_full` | `params_match_update_regret_full_in_warmup` (P1-b) |
| Checkpoint round-trip lossless | P2-a round-trip test + `smoke.sh` |
| Runtime reads what the writer wrote | V3 version gate + `load_external_blueprint` ignored test |
| A/B runs comparable | fixed `--seed`, same `--threads`, same artifacts, alternating rounds |

## APPENDIX B — EXPECTED OUTCOMES (sanity envelope)

| Metric | Before | After full Part IV | After Q-wins |
|---|---|---|---|
| it/s (8 threads, k=64) | ~27.7K | ~35–45K | −0–25% (richer abstraction costs it/s — that is fine) |
| `eval` share of traverse | ~20–35% | <5% | — |
| flush share of wall | ~30% | ~20% | — |
| infosets @100K iters | ~473K | materially fewer (river collapse) | Q1/Q3 raise it deliberately |
| `expl_mbb` @100K | baseline TBD in T0-d | better (R-bundle) | monotonically better per kept Q |
| turn precompute | days | ~1–2 h @EHS_SAMPLES=10 | minutes with Q5 |

Numbers outside these envelopes mean a gate was skipped — go back to the last green gate.

## APPENDIX C — KNOWN-ISSUES NOT FIXED HERE (logged for the next pass)

1. `eval_5`'s full-house/two-pair selection is only correct for 5-card inputs (the position-scan latent bug) — P1-a's `classify()` handles 7 cards directly, which retires the risk as long as `TableEvaluator` routes through `fast7`.
2. `pkr-fuzz::run_eval_harness` deals hole cards with `random_range(0..52)` twice per player (duplicate-card risk) and looks up `blueprint.lookup(0)` — placeholder hash. Fix: deal from a shuffled deck and compute the real infoset hash via the same `KMeansAbstraction` the trainer uses; then wire it into `fast.sh` as a behavioral smoke (status.md item 1: "play the bot").
3. `pkr-exploit` opponent-modeling overlay remains unwired (roadmap P0 lever) — independent of this playbook.
4. The GPU path (`pkr-cfr::gpu`) mirrors pre-P1-b math; if the GPU path is ever revived, port `RegretParams` into a uniform buffer and re-run the parity test.
5. `calculate_ehs` uses `rng()` (non-reproducible) — irrelevant for training (tables are precomputed) but precompute runs are not bit-reproducible; seed it from the sample index if artifact reproducibility ever matters.
