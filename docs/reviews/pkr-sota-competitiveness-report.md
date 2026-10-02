# pkr-sota — Competitiveness Report (M1 Mac mini, 16 GB)

**Scope:** `dump.txt` (213 files, ~47.5k lines: 11 Rust crates + docs/playbooks/experiments/handoffs).
**Question:** what stands between this bot and being competitive with Nash/GTO-grade play, under an M1/16 GB train-and-run constraint?

> **Read this first — limits of this review**
> - I read the core path closely: `pkr-cfr` (`traversal.rs`, `dcfr.rs`, `table.rs`, `config.rs`), `pkr-core` (`state.rs`, `abstraction.rs`), `pkr-abstraction/lib.rs`, `pkr-exploit/best_response.rs`, `pkr-export/translate.rs`, `pkr-runtime/session.rs`, `pkr-subgame` (header + blend), and ~25 docs (status, handoffs, experiments, arch). I did **not** read all 47k lines (e.g. `precompute.rs`, `pkr-fuzz`, `fast7.rs`, the 3.4k-line CI playbook).
> - **No Rust toolchain was available. Every code block below is a draft written against the signatures I saw in the dump. None of it has been compiled or run.** Treat as a design + starting patch; expect small API fixes.
> - Items marked **[verified]** are directly visible in code/docs. **[derived]** = my reasoning from code (math shown). **[hypothesis]** = plausible, needs the experiment I propose.
> - Your own `SOTA-candidates-honest.md` notes an earlier agent fabricated citations. I did a small web check for two literature calibration points (Pluribus, DecisionHoldem, below); everything else literature-related is from memory and flagged.

---

## 0. TL;DR

1. **Your measuring stick is biased in a way that punishes exactly the changes you need.** The best-response (BR) exploitability is fit and scored on the same 5k deals. Same checkpoint: **3796 mbb @5k → 1707 @20k → 1222 @40k deals** (`turn-up-investigation.md`), and the bias grows with infoset count. Consequently the headline "negative results" — T2.2 finer river, `SIG_V2` (11170 vs 5735 mbb), "30M is the sweet spot", "plateau at 3M", "k isn't the constraint" — were all decided with an estimator that **mechanically rewards smaller tables**. They are not evidence that finer abstraction has "no headroom". *(verified numbers; causal claim derived, strongly supported by your own 5k/20k/40k table)*
2. **The abstract game is not a faithful poker game.** Infoset key = `(street, actions_this_street, total_raises, last_was_bet, card-bucket)`. It cannot see **which bet size it faces** (0.5× vs 2× pot vs jam) or the pot/SPR. Facing a 0.5-pot bet and a pot-size-plus bet are the *same infoset*. That is an imperfect-recall merge that CFR has no convergence guarantee for, and it fits your flat/rising exploitability curves. Your own audit (F3) and `v40-k250-result.md` call it the binding constraint — and it was **never A/B'd** (`SIG_V3_SIZE_AWARE = false`, "gated, off"). *(verified in code; consequence is [derived]/[hypothesis])*
3. **You are using ~2% of your memory budget.** v42: ~1.2M infosets at 5M iters; ~1.26M (≈2.1% of 60M slots) at 18M. The table could hold 30–50× more. The reason given for not growing it ("splits faster than the iteration budget can fill") was measured under the biased estimator at 20M iterations. *(verified numbers)*
4. **Real-game strength has never been measured** against anything but 3 scripted bots (+194…+210 bb/100 aggregate) — which only proves it isn't broken. No LBR, no head-to-head vs frozen checkpoints at scale, no spot comparison against a solver. `status.md`: "Nothing has ever played a full game against…" (partly stale, but the gap stands).
5. **Runtime side is weakest relative to its importance.** Off-tree translation is a hard threshold (`<0.6 → 0.5×, <1.2 → 1×, else 2×`); the "pseudo-harmonic" function in `translate.rs` has **zero callers** and isn't actually pseudo-harmonic. Subgame solving is river-only, unsafe (no gadget), with tiny inner iteration counts, and its +2.40 chips/deal was measured by the same BR harness.
6. **The honest ceiling:** blueprint + good translation + safe depth-limited re-solving on an M1 can plausibly get you to "beats most bots/humans, loses to a full-tree Nash solver in spots". Matching a PioSolver-class Nash on all of HUNL is not achievable on this hardware. What *is* achievable is a bot that is **measurably** hard to exploit in the real game. Section 7 spells out what each step is worth (as hypotheses, not promises).

**Do in this order:** (A) fix measurement → (B) fix the abstract game + train bigger → (C) fix runtime translation → (D) safe depth-limited search. A–B are ~1 week of work and a few overnight runs; they re-open every conclusion currently marked "dead end".

---

## 1. The numbers that matter (from your docs)

| Fact | Value | Source |
|---|---|---|
| Training throughput (M1) | **10,228 it/s** @ `iters-per-sync 2048`, 60M cap (v36); 8.35K it/s avg incl. eval pauses | `v36-capacity-sweep.md`, handoff |
| README throughput claim | ~27,000 it/s, ~2.3B it/day | `README.md` — **stale vs 10K measured** |
| ⇒ iterations per day at 10K it/s | ≈ 880M (≈ 36.7M/hour; 200M ≈ 5.6 h) | arithmetic |
| Infosets reached | 1.23M @ 5M iters; ≈1.2%→2.1% of 60M slots @ 18M | `first-real-game-eval.md`, `turn-up-investigation.md` |
| Table memory/infoset | regret+momentum 12×i64 = 96 B + strategy-sum 6×f64 = 48 B = **144 B** (+ hash-map entry; unmeasured) | `table.rs` `RM_STRIDE=K*2`, `with_capacity` |
| Memory used | RSS ≈ 700 MB (v36), "1.4 GB at 1M infosets" | `v36`, `table.rs` comments |
| v42 curve (5k deals, in-sample BR) | 3313 @3M → 3780 @18M | `v42-post-audit-result.md` |
| Same v42-18M checkpoint vs eval deals | 3796 (5k) / 1707 (20k) / **1222 (40k)**, still falling | `turn-up-investigation.md` |
| Pre-audit "champion" | ~2170–2230 mbb (v38) — measured with estimator later found broken (F1: 8981→1748 on same data) | `status.md`, `post-audit-invalidation.md` |
| Arena vs scripted bots (v42, 5k hands) | Aggregate **+210 bb/100** (Station +258, Nit +42, Aggro +330) | `first-real-game-eval.md` |
| Hand-count sensitivity | 500 hands over-states aggregate by 65%; ~2000 minimum | same |
| `status.md` self-grade | "Tier: weak-bot… Not competitive" | `status.md` |
| River subgame POC | median −59% exploitability (uniform 12-hand ranges, check-check-check line only, 100 CFR its) | `river-subgame-poc-positive.md` |
| Street decomposition | River ≈ 40% of postflop value, turn+river 74%, flop 26% | `street-decomposition.md` |

**Calibration points from the literature (web-checked):**
- Pluribus blueprint: ~12,400 core-hours (8 days × 64 cores), <512 GB RAM, no GPU; real-time search from round 2 on.
- DecisionHoldem (open-source HUNL, arXiv 2201.11580): Linear CFR blueprint, ~200M iterations, ~4,000 core-hours on 48 cores, **plus safe depth-limited subgame solving**.
- Takeaway: **200M iterations is ≈ 5.6 h on your M1.** Iteration count is *not* your bottleneck; abstraction fidelity, the metric, and search are. (Caveat: an "iteration" is not identical across codebases.)

---

## 2. Root causes, ranked

### R1 — The metric is in-sample and its bias scales with table size  *(highest leverage)*

**[verified]** `best_response.rs::sampled_br_one_seat` fits `br_action[hash] = argmax_a Σ cfv` on `deal_seeds`, then scores the *same* `deal_seeds` (`BrResult.expl_insample_mbb`: "Biased HIGH"). With ~1–2M infosets and 5,000 deals, most infosets are seen once or twice, so the "BR" is partly *clairvoyant on those deals* (winner's curse).

**[verified]** Your measurements: 5k→20k→40k deals gives 3796→1707→1222 on one checkpoint, and `bias ~ c/deals` doesn't fit (predicts ~1707 at 40k).

**[derived]** More infosets ⇒ more free parameters in the BR ⇒ higher reading. So every experiment that **adds** infosets (T2.2, SIG_V2, k=250, capacity, longer training) is penalized *by the instrument*, and every decision to "ship coarser" is contaminated. The "turn-up at 3–6M" is the same artifact (you concluded this in the 10-01 hunt, but didn't propagate it to the *conclusions* drawn from it).

What's still valid: **paired** A/Bs at equal deal count between same-size tables (seed noise permitting). What's not: any A/B where the arms differ in infoset count.

**Fix:** S1 (held-out BR → a *lower* bound, bracketing truth with the in-sample *upper* bound), S2 (LBR in the real game), S3 (paired head-to-head protocol).

### R2 — Imperfect-recall abstract game (F3)

**[verified]** `history_signature()`:

```rust
(actions_this_street & 0xFF) | ((total_raises & 0xFF) << 8) | ((last_was_bet as u32) << 16)
```

Infoset hash = FNV-1a over `(street, len, sig bytes, cluster_id)`. Missing from the key:
- the **size** of the bet faced (0.5× / 1× / 2× / jam);
- pot / SPR;
- anything about prior streets except via `total_raises`.

So with a river bluff-catcher, "villain bets 0.5 pot" and "villain bets 2 pot" are one infoset with one mixed strategy — it must call both at the same frequency, which is wildly wrong (needs ~67% vs ~33% defense). The average strategy at that infoset is a blend of two different games' equilibria. CFR's guarantee requires perfect recall (or at least a game where the merged states have the same continuation structure); this merge breaks it, so *more iterations don't converge* — consistent with flat/rising curves at every scale you've tried.

**[verified]** The fix exists and is fingerprinted: `history_signature_v3` (street, per-action bucket sequence for this street incl. facing-size, action count, street-start pot class = SPR in equal-stack HU, raises) and `AbstractionFingerprint.sig_version = 3` when `SIG_V3_SIZE_AWARE`. It has **never been run**. `SIG_V2` (SPR only, no bet-size) *was* run and "failed" — under R1's estimator, at 20M iterations, with 3.6× infosets.

**[hypothesis]** V3 + a correct metric + 200M+ iterations is the single most likely path to a curve that actually descends.

### R3 — Capacity unused / memory layout wasteful

**[verified]** 144 B/infoset; momentum (`RM_MOMENTUM`) occupies 48 B of it and `momentum=false` by default (and docs call it "structurally wrong"). At a 12 GB ceiling (your own `arch-overview.md` §1) you can hold a few ×10⁷ infosets today; ~2× more after S5. Your own `bst.md` / instrumentation plan admit **no heap measurement exists** (`pkr-sota-instrumentation-ci-plan.md`: "per-infoset memory cost unverified") — measure before committing to a capacity.

### R4 — CFR variant is effectively "RM+ with t²-weighted average", not the DCFR you think

**[derived, from code]** `flush_cpu_batch_with` applies `update_regret_i64_mode` **once per visit item** (sorted by iteration), and `dcfr_step(t)` uses `w_pos = t^1.5/(t^1.5+1)` for `t ≥ TAU=1000`, identity before.
- Per application, the discount is `1 − t^-1.5`. Even if applied once per *iteration* (as in the paper) the cumulative product from t=1000 to ∞ is `exp(−Σ t^-1.5) ≈ exp(−2/√1000) ≈ 0.94` — i.e. old regrets are discounted ~6% **in total**.
- In MCCFR an infoset is touched in a fraction of iterations, so per-visit application discounts even *less* than per-iteration.
- Warm-up identity for t<1000 means the earliest (garbage) regrets are never discounted, which is the part of DCFR that matters.
- With `neg_floor=true` (RM+), `β=0` is dead code anyway.

Net: you're running RM+ with a recency-heavy average (`avg_power=2`). That's not wrong, but the "DCFR" label and the f32-saturation worries in the docs are moot, and the known-good MCCFR recipe for blueprints (Pluribus-style **Linear CFR**) isn't what's running. Your own data: Kuhn favors p=2, NLHE mildly favors p=1 (inconclusive). Re-test under R1's fixed metric. → S4.

### R5 — Averaging site / ε-exploration bias

- **[verified]** A stale comment in `traversal.rs` says `reach_prob` at a traverser node "is the *opponent's* reach". In the code `reach_prob` is multiplied by `strategy[a]` only on the traverser branch and passed unchanged on the opponent branch, so it is the traverser's **own** reach. The accumulation `strategy·own_reach·t^p` is the correct reach-weighted average — but **sampled** at a frequency ∝ opponent reach, i.e. it weights iterations by how often the *opponent's current* strategy reaches the infoset. External-sampling theory (Lanctot et al. 2009, from memory) accumulates at the **opponent's** nodes (`avg_at_traverser=false`) for this reason. Your F5 test said "equivalent" — measured with the biased estimator. Re-test.
- **[derived]** ε-uniform at opponent nodes (ε=0.01) means traverser regrets are computed against the **ε-perturbed** opponent and there's no importance correction ⇒ you converge to an ε-perturbed game's equilibrium. Small at 0.01, but easy to anneal. → S4b.

### R6 — Runtime: translation, search safety, and unmeasured gains

- **[verified]** `translate.rs::compute_translation` has no callers (`grep` across `crates/` + `binaries/`). Its formula `1 − (actual−lower)·reach_upper / ((upper−actual)·reach_lower + (actual−lower)·reach_upper)` is a reach-weighted linear interpolation, **not** the Ganzfried–Sandholm pseudo-harmonic mapping `f(x) = (B−x)(1+A) / ((B−A)(1+x))`.
- **[verified]** In-engine, `action_bucket` thresholds (0.6, 1.2) are hard: a 0.59-pot bet *is* a 0.5-pot bet; a 0.61 is a 1.0-pot bet. Hard thresholds are the classic exploit vector against translation.
- **[verified]** Raise sizing convention: `raise_to = opp_bet + pot·frac` where `pot` **excludes** the hero's call. A standard "pot-sized raise" is `opp_bet + (pot + to_call)`. Your "1.0×" raise is smaller than a GTO-solver "pot" raise. Internally consistent (and `action_bucket` uses the same denominator) — but you must use *your* convention when translating real opponents' sizes, and when comparing against a solver tree.
- **[verified]** Subgame solving: river-only; `bot_loop.rs` uses `iters: 10, hands_per_range: 4`; `N_CLASSES = 64` strength classes share regrets across deals (so "concrete-card" is partly abstracted again); no safe-solving gadget ("Production needs max-margin or CFRD gadget" — POC caveat #2). The headline +2.40 chips/deal (t=6.04, 20k paired deals) is measured through the same `SubgameHook` + in-sample BR harness (R1) — not by playing a different opponent.

---

## 3. Bugs & defects found

| # | Sev | Where | Finding | Status |
|---|---|---|---|---|
| B1 | **High (measurement)** | `best_response.rs` `sampled_br_one_seat` | BR fit & score on same deals; bias grows with table size; contaminates all abstraction/length conclusions | **[verified]** → S1 |
| B2 | **High (design)** | `state.rs::history_signature` | Infoset blind to bet size/pot ⇒ imperfect recall; CFR can't converge | **[verified]**; fix gated off → S6 |
| B3 | Med | `translate.rs` | `compute_translation` dead code; not pseudo-harmonic | **[verified]** → S7 |
| B4 | Med | `abstraction.rs::action_bucket` at runtime | Hard thresholds for off-tree sizes ⇒ exploitable translation | **[verified]** → S7 |
| B5 | Med | `dcfr.rs` / `table.rs::flush_cpu_batch_with` | Discount applied per visit, ~inert; "DCFR" is effectively RM+ | **[derived]** → S4 |
| B6 | Low-Med | `pkr-subgame/lib.rs::blend_p0_strategy` | **Asymmetric branches:** `(Some(sa), None)` computes `α·sa + (1−α)·sa = sa` (mass 1) while `(None, Some(sb))` returns `(1−α)·sb` (mass `1−α` — unnormalized). Safe-solve blend can emit non-distributions | **[verified]** → S8 (3-line fix) |
| B7 | Low | `traversal.rs` comment | Stale/wrong "reach_prob = opponent's reach" | **[verified]**, comment-only; but it misled F5 reasoning |
| B8 | Low | `README.md`, `arch-overview.md`, `status.md` | 27K it/s claim vs 10K measured; "no bot binary"/"unverified" sections out of date; arch doc says momentum/PCFR+ "implemented" | **[verified]** docs drift |
| B9 | Low | `dcfr.rs::update_regret_with_step` | `(x as f64 * w) as i64` truncates toward zero ⇒ every visit shaves ≤1 fixed-point unit (0.001 chip) off positive regrets. Negligible vs ±10-chip deltas; mention only because it's a systematic sign | **[derived]** |
| B10 | Info | `public_br.rs` | Self-declared WIP / wrong (clairvoyant runout). Don't wire in. Correct accelerated BR needs full public-tree enumeration | **[verified]** per its own header |
| B11 | Info | Doc hygiene | `turn-up-investigation.md` contains the exploit-shifter bug writeup and the 40k table **twice** (heredoc duplication hazard noted in handoff §5) | **[verified]** |

Already fixed per handoff (not re-reported): reader fingerprint/stride, exploit-shifter CDF decode, purify guard, stats.json env, etc.

---

## 4. Code solutions

> All blocks are **drafts against the signatures visible in the dump; uncompiled.** I name the exact file/function each hunk targets.

### S1 — Held-out (cross-fitted) BR: gives a lower bound that brackets truth

Your `br_choice` already falls back to the blueprint argmax when an infoset is missing from `br_action` — exactly the behavior needed for held-out scoring. Any fixed policy's value ≤ the true BR value, so a held-out reading is an unbiased estimate of a *lower bound*; the existing in-sample reading is an upper bound. Report both; they converge toward the truth as fit deals ≫ infosets.

`crates/pkr-exploit/src/best_response.rs`:

```diff
 fn sampled_br_one_seat(
     table: &CompactRegretTable,
     abstraction: &dyn AbstractionBuilder,
     evaluator: &dyn Evaluator,
-    deal_seeds: &[u64],
+    fit_seeds: &[u64],     // deals used to FIT br_action
+    score_seeds: &[u64],   // fresh deals used to SCORE it (may equal fit_seeds for legacy behaviour)
     br_seat: usize,
     hook: Option<&dyn SubgameHook>,
 ) -> Vec<f32> {
-    let deal_prior = 1.0 / deal_seeds.len().max(1) as f64;
+    let deal_prior = 1.0 / fit_seeds.len().max(1) as f64;
     let mut br_action: HashMap<u64, u8> = HashMap::new();

     for _iter in 0..br_iterations() {
-        let chunks: Vec<HashMap<u64, [f64; K]>> = deal_seeds
+        let chunks: Vec<HashMap<u64, [f64; K]>> = fit_seeds
             .par_iter()
             ...
     }

-    deal_seeds
+    score_seeds
         .par_iter()
         .map(|&seed| { /* unchanged walk_fixed scoring */ })
         .collect()
 }
```

Update the existing caller(s) to pass `(&seeds, &seeds)` to keep legacy behaviour, then add:

```rust
/// Held-out result. `lower_mbb` is a statistical LOWER bound on abstract-game
/// exploitability (BR fit on `fit_deals`, scored on disjoint `score_deals`;
/// unseen infosets fall back to the blueprint's argmax, i.e. no exploitation).
/// `insample_mbb` (fit==score) is the UPPER-biased legacy reading.
#[derive(Debug, Clone, Copy)]
pub struct HeldOutBr {
    pub lower_mbb: f64,
    pub lower_se_mbb: f64,
    pub insample_mbb: f64,
    pub fit_deals: u32,
    pub score_deals: u32,
}

pub fn heldout_exploitability(
    table: &CompactRegretTable,
    abstraction: &dyn AbstractionBuilder,
    evaluator: &dyn Evaluator,
    fit_deals: u32,
    score_deals: u32,
    seed: u64,
) -> HeldOutBr {
    // Disjoint seed streams: fit and score never share a deal.
    let mut rng = SmallRng::seed_from_u64(seed);
    let fit: Vec<u64> = (0..fit_deals).map(|_| rng.random::<u64>()).collect();
    let score: Vec<u64> = (0..score_deals).map(|_| rng.random::<u64>()).collect();

    let v0 = sampled_br_one_seat(table, abstraction, evaluator, &fit, &score, 0, None);
    let v1 = sampled_br_one_seat(table, abstraction, evaluator, &fit, &score, 1, None);

    // Same deal (seed) is scored for both seats ⇒ pair them to cut variance.
    // BB = 2 chips ⇒ 1 chip = 500 mbb. (Match whatever sampled_exploitability uses.)
    let per_deal: Vec<f64> = v0.iter().zip(&v1).map(|(a, b)| 0.5 * (*a as f64 + *b as f64) * 500.0).collect();
    let n = per_deal.len().max(1) as f64;
    let mean = per_deal.iter().sum::<f64>() / n;
    let var = per_deal.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0).max(1.0);

    let ins = sampled_exploitability(table, abstraction, evaluator, fit_deals, seed);
    HeldOutBr {
        lower_mbb: mean,
        lower_se_mbb: (var / n).sqrt(),
        insample_mbb: ins.exploitability_mbb,
        fit_deals,
        score_deals,
    }
}
```

**How to use it:** for one checkpoint, sweep `fit_deals ∈ {10k, 40k, 160k}` with `score_deals = 20k`. The held-out **rises** and the in-sample **falls** as fit grows; the gap is your measurement uncertainty. Adopt `lower_mbb` as the training-curve metric (it is monotone-friendly to bigger tables, unlike the legacy one). Wire `--eval-mode heldout` into the trainer's `--eval-every`.

### S2 — Local Best Response (LBR) against the *deployed* policy, real-game actions

Why: the abstract-game BR can't see off-tree sizes or translation, and it is the wrong object — you will be playing the real game. LBR (Lisý & Bowling 2017, from memory) is a cheap, sound **lower bound** on real exploitability: it plays a greedy, myopic best response using equity vs the posterior range and the opponent's blueprint fold/call probabilities, and may use **bet sizes your abstraction doesn't contain**.

Draft `crates/pkr-exploit/src/lbr.rs` (uses your `RangeTracker`, `legal_actions_into`, `terminal_payoff`):

```rust
//! Local Best Response vs the deployed policy. Lower bound on real exploitability.
use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::{Action, ActionKind, GameState};
use pkr_subgame::range_tracker::{index_of_hole, RangeTracker};
use rand::{rngs::SmallRng, Rng, RngExt, SeedableRng};

const K: usize = 6;
/// Verify against terminal_p0_fast(): per your river-hash comment, LOWER value = stronger hand.
const LOWER_IS_BETTER: bool = true;

fn combos() -> Vec<[u8; 2]> {
    let mut v = Vec::with_capacity(1326);
    for a in 0..52u8 { for b in (a + 1)..52u8 { v.push([a, b]); } }
    v
}
fn bucket(s: &GameState, k: &ActionKind) -> usize {
    let a = s.actor;
    pkr_core::abstraction::action_bucket(k, s.stacks[a], s.street_bets[a], s.street_bets[1 - a], s.pot) as usize
}
fn legal(s: &GameState) -> ([Action; 8], usize) {
    let mut buf = [Action { player: 0, kind: ActionKind::Fold }; 8];
    let n = s.legal_actions_into(&mut buf);
    (buf, n)
}
/// Blueprint average strategy at `s` for the actor, masked to legal buckets.
fn policy(tbl: &CompactRegretTable, abs: &dyn AbstractionBuilder, s: &GameState) -> [f32; K] {
    let (buf, n) = legal(s);
    let mut cnt = [0usize; K];
    for a in &buf[..n] { cnt[bucket(s, &a.kind)] += 1; }
    let mut sig = [0u8; 8];
    let l = s.infoset_signature_into(&mut sig);
    let board = &s.board[..s.board_len as usize];
    let h = abs.get_infoset_hash(&s.hole[s.actor], board, &sig[..l], s.street as u8);
    let mut p = [0f32; K];
    tbl.get_average_strategy_into(h, &mut p);
    let mut t = 0.0;
    for a in 0..K { if cnt[a] == 0 { p[a] = 0.0 } else { t += p[a] } }
    if t > 1e-9 { for a in 0..K { p[a] /= t } }
    else {
        let nl = cnt.iter().filter(|&&c| c > 0).count().max(1) as f32;
        for a in 0..K { p[a] = if cnt[a] > 0 { 1.0 / nl } else { 0.0 } }
    }
    p
}
fn showdown(ev: &dyn Evaluator, me: &[u8; 2], opp: &[u8; 2], board5: &[u8]) -> f32 {
    let (a, b) = (ev.evaluate_hand(me, board5), ev.evaluate_hand(opp, board5));
    if a == b { 0.5 } else if (a < b) == LOWER_IS_BETTER { 1.0 } else { 0.0 }
}

/// Greedy LBR action. EV in final chips (net), check-down assumed after our action;
/// opponent re-raises are scored as us folding (conservative ⇒ still a lower bound).
fn lbr_choose(
    s: &GameState, me: usize, tr: &RangeTracker,
    tbl: &CompactRegretTable, abs: &dyn AbstractionBuilder, ev: &dyn Evaluator,
    extra_pot_fracs: &[f32],            // OFF-TREE sizes, e.g. [0.33, 0.75, 1.5, 3.0]
    rng: &mut SmallRng, n_samples: usize,
) -> Action {
    let opp = 1 - me;
    let board: Vec<u8> = s.board[..s.board_len as usize].to_vec();
    let my = s.hole[me];
    let post = tr.range(opp as u8);
    let cs = combos();
    // posterior-weighted CDF over opponent hands, masking blockers
    let mut cdf = Vec::with_capacity(cs.len());
    let mut acc = 0.0f64;
    for h in &cs {
        let blocked = h.iter().any(|c| my.contains(c) || board.contains(c));
        acc += if blocked { 0.0 } else { post[index_of_hole(h)] };
        cdf.push(acc);
    }
    let (buf, n) = legal(s);
    let inv = s.total_invested[me];
    let pot = s.pot;
    // candidates = legal actions + off-tree bets/raises (pot-fraction of CURRENT pot, clamped legal-ish)
    let mut cands: Vec<Action> = buf[..n].to_vec();
    let base = s.street_bets[me];
    let opp_bet = s.street_bets[opp];
    for &f in extra_pot_fracs {
        let amt = (opp_bet.max(base) + pot * f).min(s.stacks[me] + base);
        if amt > opp_bet + 1e-6 { cands.push(Action { player: me, kind: ActionKind::Bet(amt) }); }
    }
    if acc <= 0.0 { return buf[..n].iter().find(|a| matches!(a.kind, ActionKind::Check | ActionKind::Call)).copied().unwrap_or(buf[0]); }

    // draw (opp hand, runout) samples once; reuse across candidates (common random numbers)
    let need = 5 - board.len();
    let mut samples: Vec<([u8; 2], [u8; 5])> = Vec::with_capacity(n_samples);
    for _ in 0..n_samples {
        let r = rng.random::<f64>() * acc;
        let hi = cdf.partition_point(|&c| c < r).min(cs.len() - 1);
        let h = cs[hi];
        let mut dead = [false; 52];
        for c in my.iter().chain(h.iter()).chain(board.iter()) { dead[*c as usize] = true; }
        let mut full = [0u8; 5];
        full[..board.len()].copy_from_slice(&board);
        let mut k = board.len();
        while k < 5 { let c = rng.random_range(0..52u8); if !dead[c as usize] { dead[c as usize] = true; full[k] = c; k += 1; } }
        let _ = need;
        samples.push((h, full));
    }

    let mut best = (f32::NEG_INFINITY, cands[0]);
    for cand in cands {
        let ev_c: f32 = match cand.kind {
            ActionKind::Fold => -inv,
            ActionKind::Check | ActionKind::Call => {
                let c = (opp_bet - base).max(0.0);
                samples.iter().map(|(h, b5)| showdown(ev, &my, h, b5) * (pot + c) - (inv + c)).sum::<f32>() / n_samples as f32
            }
            ActionKind::Bet(total) => {
                let x = total - base;                       // chips we add
                let d = (total - opp_bet).min(s.stacks[opp]);// chips opp adds to call
                let mut ps = s.clone();
                ps.apply_action_in_place(&cand);
                let mut tot = 0.0f32;
                for (h, b5) in &samples {
                    ps.hole[opp] = *h;
                    let p = if ps.is_terminal() || ps.actor != opp { [0.0, 1.0, 0.0, 0.0, 0.0, 0.0] } else { policy(tbl, abs, &ps) };
                    let (pf, pc) = (p[0], p[1]);
                    let pr = (1.0 - pf - pc).max(0.0);
                    let o = showdown(ev, &my, h, b5);
                    tot += pf * (pot - inv)
                         + pc * (o * (pot + x + d) - (inv + x))
                         + pr * (-(inv + x));
                }
                tot / n_samples as f32
            }
        };
        if ev_c > best.0 { best = (ev_c, cand); }
    }
    best.1
}
```

The driver (per hand: shuffle, deal, alternate seat, loop `if actor==me {lbr_choose} else {sample bucket from policy(); pick first concrete action in that bucket}`, apply to both `state` and `tracker`, `advance_street` on both when `is_street_complete()`, accumulate `state.terminal_payoff(me, ev)`) mirrors `walk_fixed`/`bot_loop.rs`; use 20k+ hands, both seats, **duplicate deals** (same cards, seats swapped), report mean ± SE in mbb/g.

Notes: (1) pass `extra_pot_fracs = [0.33, 0.75, 1.5, 3.0]` — LBR then punishes bad translation; with V1 signatures off-tree bets silently merge into in-tree infosets, which this will expose. (2) `n_samples` 300–600 is enough for a greedy policy. (3) Run it first on the **blueprint-only** policy, later on blueprint+subgame to measure what search buys *without* the in-sample BR.

### S3 — Head-to-head protocol that can actually rank checkpoints

Use `pkr-fuzz::tournament` (exists, "scripted-bot wiring untested end to end" per `status.md`) with:
- **Duplicate poker**: every deal played twice with seats swapped. This removes card luck, typically the dominant variance source.
- Your own sensitivity table says ≥ 5,000 hands per quoted number; with duplicate deals aim for 20k+.
- Pairwise matrix among: v42-3M, v42-18M, each new V3 run, + (S10) an external reference.
- Report mbb/hand ± SE. Gate decisions on **paired** differences.
- Add a `--ckpt-dir` policy: **save distinct checkpoints per eval point** (`train.ckpt` is overwritten, which is why "3M vs 18M" can't be tested today — `turn-up-investigation.md` §2).

### S4 — Training-algorithm patches (A/B, don't assume)

**S4a. Linear-CFR option (Pluribus-style) with exact, overflow-safe weights.** Weight each regret increment by `t` instead of discounting old regret — mathematically the same "linear" weighting without any sweep:

`crates/pkr-cfr/src/config.rs`:
```diff
+    /// Linear MCCFR: regret increments weighted by t/1e6 (scale keeps i64 headroom),
+    /// average-strategy weight t (set avg_power=1 for consistency). Disables per-visit DCFR discount.
+    pub linear_cfr: bool,
 ...
 impl Default: linear_cfr: false,
 ...
 from_env: linear_cfr: env_bool("PKR_LINEAR_CFR", d.linear_cfr),
```
`crates/pkr-cfr/src/dcfr.rs`, top of `dcfr_step`:
```rust
if crate::config::TrainConfig::global().linear_cfr {
    return DcfrStep { w_pos: 1.0, w_neg: 1.0, gamma: 0.0, identity: iteration == 0 };
}
```
`crates/pkr-cfr/src/traversal.rs`, where `BatchItem` is pushed:
```diff
-            let delta = v[a] - v_sigma;
+            let mut delta = v[a] - v_sigma;
+            if crate::config::TrainConfig::global().linear_cfr {
+                delta *= global_iteration as f32 / 1.0e6;   // ≤ 200 at 200M iters
+            }
```
Overflow check: |Δ| ≤ ~200 chips → ×1000 fixed = 2e5; ×200 weight = 4e7 per visit; ≤2e8 visits ⇒ < 8e15 ≪ `R_MAX = i64::MAX/4 ≈ 2.3e18`. ✔ (Revisit if you push past ~1B iterations.)

A/B arms (all at the *same* eval metric, S1): `{RM+ p=2 (current)} × {linear_cfr, p=1} × {neg_floor on/off}`. Note: weighted-increment RM+ is "CFR+ with linear weighting", not literally Pluribus' no-floor Linear CFR — include `PKR_RM_PLUS=0` as an arm.

**S4b. Anneal exploration instead of fixed ε:**
```rust
// traversal.rs
fn exploration_epsilon_at(t: u32, total: f32) -> f32 {
    let base = crate::config::TrainConfig::global().explore_epsilon.max(0.002);
    let frac = (t as f32 / total).clamp(0.0, 1.0);
    // 5× base early (reachability), decays to base/5 (bias → 0)
    (5.0 * base) * (1.0 - frac) + (base / 5.0) * frac
}
```
Pass `total` through `TrainConfig::hs_dcfr_total`-style env (`PKR_TOTAL_ITERS`).

**S4c. Re-test the averaging site** (`PKR_AVG_AT_TRAVERSER=0`) under S1. Theory favors it (R5); your F5 "equivalent" came from the biased estimator.

### S5 — Reclaim memory (≈33–50%) and measure it

1. Drop the momentum plane (it's off and "structurally wrong"):
```diff
-const RM_FIELDS: usize = 2;
+const RM_FIELDS: usize = 1;       // regret only
-const RM_MOMENTUM: usize = 1;
```
…and delete the `store_rm(.., RM_MOMENTUM, ..)` writes/reads in `flush_cpu_batch_with` (the `mom_i64` variables become `0`). Bump the checkpoint version (`ckpt_v7` → v8) so old files fail loudly rather than mis-stride. Saves 48 B/infoset → ~96 B (regret i64 48 + strat f64 48).
2. Second step (optional, A/B): store regrets as **f32** + strategy sums as **f32 with periodic per-infoset renormalization** → ~48 B. Only after (1) proves out; strategy-sum precision with `t²` weights is the risk (the code comment on f64 sums explains why fixed-point failed before).
3. **Measure, don't assume**: add a `--mem-report` that prints `allocated() × 96 B`, plus `/usr/bin/time -l` max RSS and the papaya map's `len()`; your own CI plan admits there's no heap measurement. Budget: keep training RSS ≤ 12 GB (`arch-overview.md`).
4. Rough capacity (estimate; map overhead unmeasured, assumed 40–50 B/entry): 12 GB / (96+50) ≈ **80M infosets** after step 1 vs ~60M before. You are at ~1–2M.

### S6 — Make the abstract game a real game: enable V3, strengthen it, re-test at scale

Step 1 (zero code): `crates/pkr-core/src/state.rs`
```diff
-pub const SIG_V3_SIZE_AWARE: bool = false;
+pub const SIG_V3_SIZE_AWARE: bool = true;
```
The fingerprint already writes `sig_version = 3`, so mismatched checkpoints will refuse to load. ✔

Step 2 — two cheap improvements to V3 (hypotheses; A/B them):
- Finer pot class: half-octaves instead of whole log2 steps (SPR resolution matters a lot postflop).
- Remember the **previous-street aggressor** (who bet last on the prior street) — a 1-bit proxy for range polarity.

```rust
// state.rs — inside history_signature_v3 (bit budget: 19..22 raises, 22..23 new aggressor, 14..19 pot class)
let pot_bb = (self.street_start_pot / 2.0).max(1.0);
let pot_class = ((pot_bb.log2() * 2.0).floor() as u64).min(15) & 0x1F;   // half-octave, 5 bits (was 4)
// layout: street(0..3) | seq(3..10) | n_actions(10..13) | pot_class(13..18) | raises(18..21) | prev_aggr(21) | ver(60..64)
let prev_aggr = self.prev_street_aggressor() as u64 & 1;                  // add: track in apply_action_internal
```
(Add `prev_street_aggressor: u8` to `GameState` + `UndoRecord`; set when a Bet is applied, snapshot at street advance. Mirror in `undo_action`. This touches the undo stack — add a test next to `c1_tests`.)

Step 3 — the experiment, now at the scale your budget supports:

| arm | sig | iters | capacity | metric |
|---|---|---|---|---|
| A | V1 (current) | 200M | 60M | S1 held-out + S2 LBR |
| B | V3 | 200M | 60M | same |
| C | V3 + half-octave pot + aggr | 200M | 60M | same |

≈ 5.6 h/arm at 10K it/s (less if throughput holds with larger tables; README says it drops past ~3 GB — measure). **Save a checkpoint every 25M to distinct paths** and score each with S1 to *see the curve*. Decision rule: ship the arm with the best LBR + held-out; require ≥2 seeds for any claim < 2×SE.

Expectation to test (hypothesis): B/C's curve should descend where A's flattens. If B is also flat under S1, R2 is wrong and I'd look next at R4/R5 and the 2D EHS/EHS² features.

### S7 — Runtime translation: pseudo-harmonic, randomized, consistent with the tracker

Replace the hard thresholds *at runtime only* (training still uses `action_bucket` because it only generates in-tree sizes). Sizes are in **your** convention (raise-above-call over pre-call pot), expressed as a fraction `x`:

```rust
// crates/pkr-runtime/src/translate_live.rs
/// Ganzfried–Sandholm pseudo-harmonic mapping. Returns P(map to smaller size A) for A ≤ x ≤ B.
/// f_A(x) = (B - x)(1 + A) / ((B - A)(1 + x))
pub fn pseudo_harmonic_prob_lower(a: f32, b: f32, x: f32) -> f32 {
    if x <= a { return 1.0; }
    if x >= b { return 0.0; }
    ((b - x) * (1.0 + a)) / ((b - a) * (1.0 + x))
}

/// Map an observed bet fraction to an abstract bucket (2,3,4 = 0.5x,1x,2x; 5 = jam), RANDOMIZED.
/// `jam_frac` = stack / pot at this node, so large bets blend toward the jam bucket.
pub fn translate_bet<R: rand::Rng>(x: f32, jam_frac: f32, rng: &mut R) -> u8 {
    const SIZES: [(f32, u8); 3] = [(0.5, 2), (1.0, 3), (2.0, 4)];
    if x >= jam_frac * 0.999 { return 5; }
    if x <= SIZES[0].0 { return SIZES[0].1; }
    let mut sizes: Vec<(f32, u8)> = SIZES.to_vec();
    if jam_frac > 2.0 { sizes.push((jam_frac, 5)); }
    for w in sizes.windows(2) {
        let ((a, ba), (b, bb)) = (w[0], w[1]);
        if x >= a && x <= b {
            let p = pseudo_harmonic_prob_lower(a, b, x);
            return if rng.random::<f32>() < p { ba } else { bb };
        }
    }
    sizes.last().unwrap().1
}
```
Integration rules (these matter more than the function):
1. **Sample once per decision and commit.** Feed the *same mapped bucket* into the infoset hash, the `RangeTracker.apply_action`, and any subgame root — otherwise the tracker's posterior and the blueprint's history diverge.
2. For V3 signatures the mapped bucket is what's written into `abstract_history`, so off-tree bets must be rewritten as the sampled in-tree action before hashing (apply a synthetic `Bet(mapped_amount)` to the *abstract* state, keep the real state for chips).
3. Delete or rewire `translate.rs::compute_translation` (dead + mislabelled). Keep its tests only if you use it.
4. Re-assess the sizes: with 6 buckets and `BET_SIZINGS = [0.5, 1, 2]`, add a 0.33 (small-bet/probe, important on dry flops) and a ~1.5 only after S6 shows the key is stable; each new size multiplies tree width. Use LBR (S2) with off-tree fractions to decide **empirically** where the holes are.
5. Clarify the sizing convention in `abstraction.rs` doc comments and add a test pinning "pot-size raise ≠ solver pot-size raise" so nobody "fixes" it silently.

### S8 — Subgame solver fixes

**B6 fix** (`pkr-subgame/src/lib.rs::blend_p0_strategy`): make the missing-side arms symmetric and normalized — missing side ⇒ use the other side unchanged (or uniform; pick one and test):

```rust
match (a[i], b[i]) {
    (None, None) => out.push(None),
    (Some(sa), None) => out.push(Some(sa)),   // was α·sa + (1-α)·sa (== sa): make intent explicit
    (None, Some(sb)) => out.push(Some(sb)),   // was (1-α)·sb  ⇒ sums to 1-α, not a distribution
    (Some(sa), Some(sb)) => {
        let mut s = [0.0; ABSTRACT_BUCKETS];
        for k in 0..ABSTRACT_BUCKETS { s[k] = alpha * sa[k] + (1.0 - alpha) * sb[k]; }
        out.push(Some(s));
    }
}
```
Add a property test: `∀α∈[0,1], Σ out = 1`.

**Make it safe** (design, not a drop-in): your own POC doc lists "No safe-solving constraint" as caveat #2. Minimal safe-ish scheme that fits an M1:
1. At subgame root, compute the opponent's **counterfactual best-response values (CBVs)** per hand against the *blueprint* continuation (you already have a BR walker and the `RangeTracker`).
2. Build the resolving gadget (Burch/Johanson/Bowling CFR-D; Brown & Sandholm reach-subgame): for each opponent hand, a *terminate* option paying that CBV vs *enter*; solve with the same flat-tree CFR. This bounds exploitability at the blueprint's level (up to CBV estimation error).
3. Replace "10 inner iterations, 4 hands" (`bot_loop.rs`) by a wall-clock budget (2–5 s is fine for an offline bot) and measure convergence on the subgame itself (internal exploitability of the solved subgame must fall).
4. Un-share the 64 strength classes (`N_CLASSES`) on small trees — it re-abstracts what you wanted concrete.
5. Extend to the **turn** only after (a) leaf values exist (below) and (b) S2 shows turn is where LBR extracts the most.

**Depth-limited leaves via biased continuation strategies (Pluribus/Modicum idea; no retraining):** at export, derive 3 extra "styles" from the same blueprint by multiplying action probabilities and renormalizing — cheap and testable:

```rust
pub fn bias_strategy(p: &[f32; 6], bucket_mult: &[f32; 6]) -> [f32; 6] {
    let mut o = [0.0; 6]; let mut t = 0.0;
    for i in 0..6 { o[i] = p[i] * bucket_mult[i]; t += o[i]; }
    if t > 0.0 { for x in &mut o { *x /= t } } else { return *p; }
    o
}
// fold-biased:  [5,1,1,1,1,1]   call-biased: [1,5,1,1,1,1]   raise-biased: [1,1,5,5,5,5]
```
At a depth-limited leaf (end of turn betting), the opponent picks among {blueprint, fold-, call-, raise-biased} continuations → robust leaf values. This is the part of "real-time search" that makes the blueprint's coarse river abstraction stop mattering as much.

### S9 — Better "GTO proxy" tests without a commercial solver

You asked for competitiveness vs Nash/GTO. You can't query PioSolver here, but you can build a **spot-EV-loss benchmark**:
1. Pick ~200 river and ~100 turn spots (board, ranges from your own blueprint reach, stack/pot fixed).
2. Solve each with your **concrete-card CFR** at high iteration count (or an open-source postflop solver such as `postflop-solver` — I believe it's an open-source Rust solver, but verify availability/licence).
3. Measure the deployed policy's EV loss vs the solved strategy in each spot (BR-of-opponent value difference). Report mbb/spot, weighted by reach.
4. This is the cleanest "distance to GTO" number available to you, and it tells you whether turn/river re-solving is worth the engineering.

Also play a **public reference bot** if you can match stack depth (your tables are 100 bb, 1/2 blinds). I'd verify the stack depth of any external opponent before comparing.

---

## 5. Experiment plan (M1 budget)

| # | What | Cost (M1) | Decides |
|---|---|---|---|
| E0 | Implement S1, S3 (distinct checkpoints), run S1 on v42 final at fit=10k/40k/160k | 0.5–1 day dev + ~1 h | Whether R1's bracket is tight; re-baseline |
| E1 | Re-score existing evidence: v33 rich-preflop, T2.2, SIG_V2, k=250 *where checkpoints survive* (else re-run short) | few hours | Which "negative results" survive R1 |
| E2 | S2 LBR on v42 blueprint (+ extra off-tree sizes) | ~1 day dev + ~1 h | First real-game exploitability bound; translation holes |
| E3 | S6 arms A/B/C × 200M (+ ckpt every 25M), 1 seed first | ~17 h total | R2 (is the key the bottleneck?) |
| E4 | S4a/S4c arms on the winner of E3, 100M each | ~6 h × arms | Linear vs RM+, averaging site |
| E5 | S7 translation in the arena + LBR with off-tree sizes | 1–2 days dev | Real-game robustness |
| E6 | S8 safe river re-solve with budgeted time; measure with LBR & S3 | 1–2 weeks | Whether search adds real strength |
| E7 | S9 spot-EV-loss vs solved spots | 2–3 days | Distance-to-GTO; turn go/no-go |

**Process rules** (your handoff hazards still apply): launch with `mkdir` locks; write whole files with `>` not heredoc `>>`; export `PKR_CENTROID_FEATURE_V` to match training; one variable per run; ≥2 seeds before believing anything under ~2×SE.

---

## 6. What to stop doing / stop believing

- **Stop quoting absolute exploitability from the in-sample BR.** Your docs already call them "upper bounds"; after S1 quote the held-out lower bound *and* in-sample upper bound.
- **Stop treating "30M sweet spot" and "plateau-stop at 3–5 evals" as settled** — plateau-stop with the in-sample metric will *systematically kill* the bigger-table runs. Turn it off until S1 is in.
- **Stop expanding the 13-playbook/CI infrastructure** (`pkr-sota-instrumentation-ci-plan.md` is 3,386 lines; the improvement playbook 1,989) until the three measurements above exist. The codebase has more process than measurement.
- **Don't wire `public_br.rs`** (its own header says it's wrong). If you want a *fast* exact BR later, write the public-tree range walker properly (Johanson et al.) — S1+S2 are far cheaper first.
- **Opponent-pool/exploit pivot** (`novel-directions.md` #1): fine as a later layer, but not before a sound base — beating `StationBot`/`AggroBot` (+258/+330 bb/100) says almost nothing, and an exploit layer on a biased-key blueprint inherits its leaks. (Also: the exploit shifter bug fixed on 10-02 means any pre-fix exploit-layer results are void.)

---

## 7. Realistic expectations vs "Nash GTO" (hypotheses, not measurements)

| Milestone | What it would mean | Evidence needed |
|---|---|---|
| M0 (now) | Beats scripted bots; real-game exploitability **unknown** (abstract-BR upper bound ≳1.2k mbb @40k deals, still falling) | — |
| M1 | V3 key + held-out curve that **descends** with iterations; LBR number exists | E0–E3 |
| M2 | Translation robust: LBR with off-tree sizes ≈ LBR with in-tree sizes | E5 |
| M3 | Safe river (then turn) re-solving shows gains in **LBR and duplicate H2H**, not just in-sample BR | E6 |
| M4 | Spot-EV-loss vs solved spots within a small fraction of pot on river, moderate on turn | E7 |

What I would **not** promise: parity with a full-tree Nash solver on all HUNL nodes. Even DecisionHoldem-class systems get their strength from real-time search layered on a blueprint, and the published blueprint budget (≈4,000 core-hours) is within reach of your M1 only in *iteration count*, not in memory for a rich abstraction + 6–8 bet sizes. What is realistic is a bot that is *provably not trivially exploitable* in the real game and has solid river/turn play — a step-change from "weak-bot / unknown".

---

## 8. Appendix

### A. Why the bias argument is airtight enough to act on
The BR policy is a function of `hash` (abstract infoset). It's fit as `argmax_a Σ_{fit deals} cfv`. If an infoset appears in *n* fit deals, argmax over noisy sums picks the action that was luckiest on those deals; scoring on the same deals then includes that luck. Expected optimism ≈ E[max of K noisy sums] − max E[·], growing with the *number of infosets with small n*. Your 5k→20k→40k measurements show it directly. Held-out scoring removes the luck term; falling back to the blueprint action on unseen infosets removes the free parameters, giving a valid lower bound.

### B. Memory/iteration arithmetic
- 10.2K it/s ⇒ 36.7M it/h ⇒ 200M ≈ 5.5 h ⇒ 1B ≈ 27 h.
- Current layout: 144 B/infoset (+map). After S5(1): ~96 B. 12 GB budget ⇒ roughly 60–80M infosets (map overhead unmeasured).
- You currently populate ~1–2M slots; V3 will multiply this (SIG_V2 alone gave 3.6×; V3's per-street action sequence is larger) — still far below budget.

### C. Items I'd double-check first when you apply this
1. `LOWER_IS_BETTER` in S2 against `terminal_p0_fast`.
2. Whether `GameState.hole`, `total_invested`, `stacks`, `street_bets` are `pub` (S2 assigns `ps.hole[opp]`).
3. The mbb conversion in S1 (`× 500` assumes BB = 2 chips) vs `sampled_exploitability`.
4. `RangeTracker::range()` blocker handling for the hero's own cards (S2 re-masks anyway).
5. Checkpoint format bump (S5) — `ckpt_v7_roundtrip_beyond_i32` and `ckpt_v6_rejected` tests show the pattern to extend.

### D. Stale/misleading docs to fix (cheap credibility win)
- `README.md`: it/s claim, "source not in snapshot" sections, test counts.
- `arch-overview.md` §3.1 still lists PCFR+ momentum as part of the algorithm.
- `docs/status.md`: "No bot binary / nothing has played" vs the arena results; date 09-30.
- `traversal.rs` comment on `reach_prob` (B7).
- Deduplicate `turn-up-investigation.md` (B11).
