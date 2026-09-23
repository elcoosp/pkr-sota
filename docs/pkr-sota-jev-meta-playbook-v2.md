# pkr-sota × Jev — Meta-Improvement Playbook & Integration Brief (v2)

> **v2 changes:** new §5.3 field-by-field provenance map (every state field → exact
> crate/file/symbol, EXISTS / ADAPTER / NEW), corrected heads-up state schema, concrete JT1
> builder code over real engine types (`GameState`, `SotaAdvice`, `calculate_ehs`), and
> provenance-corrected M3/M4 cards. Everything else carries over from v1.
>
> Companion to `download/pkr-sota-M1-playbook.md` (the CFR/abstraction/training playbook).
> That playbook makes the blueprint **better and faster on your Mac Mini M1 16GB**.
> This playbook adds the **meta layer**: how to plug TypeSafe AI's **Jev** model around the
> engine so the bot wins **tournaments**, not just solves subgames.
>
> Written so a "dumb AI agent" can follow it: exact files, exact JSON/Rust, exact verify commands.

---

## 0. TL;DR

- **What Jev is (verified):** TypeSafe AI's "System One" model (launched Sep 15, 2026, $40M seed).
  You POST `{model, state, questions}`; it answers with **typed decisions** —
  `Noul` (yes/no probability), `Choice` (one of ≤255 options + full probability distribution +
  `confidence`), `Score` (2–10 described levels + weighted score + `confidence`).
  All questions are evaluated **in parallel and in isolation** against the same state in one call.
  ~100 ms typical (70–500 ms), **$0.042 / 1M input tokens, output free**.
- **The one-sentence doctrine:** **Jev decides *about* the engine; CFR decides *at* the table.**
  Jev never picks the poker action from raw cards. It classifies regime, opponents, spots, and
  off-tree actions, and routes/parameterizes solver-grade artifacts (blueprints, exploit presets,
  re-solve triggers). Your M1 stays the source of truth; Jev adds 0 bytes of local RAM.
- **13 concrete meta improvements** (§4) in 4 tiers: runtime meta-layer, tournament/meta-game edge,
  training-time labeling, dev workflow. Every card names the exact crate/file it touches.
- **The state to send Jev** (§5): a compact, pre-computed JSON the Rust code builds — numbers and
  bucket IDs, *not* raw math (Jev cannot do arithmetic; documented limitation). **New in v2:**
  §5.3 is a field-by-field provenance map — every state field traced to the exact crate/file/
  symbol that produces it (`GameState`, `calculate_ehs`, `SolverHandle::lookup`→`SotaAdvice`,
  `compute_translation`, …), marked EXISTS / ADAPTER / NEW.
- ⚠️ **Heads-up scope (v2):** `GameState` is a 2-player structure — the engine is HU-NLHE. The
  meta layer is specified for heads-up play (one `villain` object); tournament context arrives
  via a new feed-side `TournamentContext` struct, never through `pkr-core`.
- **Ready-to-paste question packs** (§6): one runtime pack (~13 questions, one call per decision,
  speculative fan-out), one training-triage pack, one eval/calibration pack.
- **Rollout:** shadow mode → parity mode → confidence-gated live, with a kill switch env var and
  acceptance tests measured in `pkr-testgames` / `pkr-exploit` (mb/hand with CI, Brier calibration).
- **Cost/latency:** ~$0.00007 per runtime decision (~$0.70 per 10k-hand session), ~100–300 ms added
  latency inside online timebanks, **zero impact on the M1 training loop** (Jev is never in the
  CFR hot path).
- ⚠️ **Compliance:** automated play violates the ToS of most real-money sites. Use this stack where
  it is permitted: simulators, home leagues, study tools, sanctioned bot events, or your own
  research environments. Nothing in this playbook should be pointed at a site that forbids bots.

---

## 1. What Jev actually is — verified research digest

Everything below is cross-checked across typesafe.ai, docs.typesafe.ai, and independent deep-dives
(flaviocopes.com/jev, DataCamp, LangChain blog, mindstudio, requesty, madewithjev catalog).

### 1.1 Fact table

| Property | Verified value | Why it matters for pkr-sota |
|---|---|---|
| Model | `jev-latest` (stable), `jev-1.13.0` current, `jev-preview` | Pin the exact version once thresholds are tuned |
| Training | **RLCD** — Reinforcement Learning for Calibrated Decisions | Probabilities aim to be *calibrated* (a 0.9 answer is right ~90% of time) — usable as real probabilities |
| Primitives | `Noul` (yes/no → `noul` 0–1), `Choice` (≤255 options → `choice` + `probabilities` + `confidence`), `Score` (2–10 levels → weighted `score` + `legend` + `probabilities` + `confidence`) | Three shapes cover regime routing, archetype ID, spot gating |
| Parallelism | All questions in one call, evaluated independently & in isolation; adding questions ≈ free latency | Ask *everything you might need* per decision (speculative fan-out) |
| State | String, JSON object, or array; **text only**; ~64k tokens total; state + longest question ≤ ~32k tokens | Pre-compute and filter in Rust; send bucket IDs and facts, not raw card math |
| Confidence | Derived from the shape of `probabilities` (1.0 = all mass on one option); returned on Choice/Score, **not** on Noul | Your gating thresholds: act / blend / fallback |
| Latency | 70–500 ms end-to-end, most ~100 ms (US-West; add network RTT) | Fits online timebanks; useless inside the CFR traversal loop — keep it out |
| Price | $0.042 per 1M input tokens, output free; ~300-token call ≈ $0.0000126 | Runtime meta-layer is effectively free; training-time labeling ≈ $50 per 1M hands |
| Rate limits | 250k tokens/s, 1,200 req/min (early access, dynamic) | Comfortably above tournament decision rates |
| SDKs | Python `typesafe-sdk`, JS `@typesafe-ai/sdk`, Vercel AI SDK `experimental_evaluate` (`@ai-sdk/typesafe-ai`, Gateway id `typesafe-ai/jev`) | For Rust you call the REST API directly (§7 JT0) — 150 lines |
| Docs | docs.typesafe.ai (Mintlify: append `.md` to any URL; `/llms.txt` index); evals.typesafe.ai; console.typesafe.ai | Give these URLs to your coding agent up front |
| Agent skill | `npx skills add typesafe-ai/skills --skill typesafe-ai` (or `claude plugin install typesafe@typesafe-ai`) | Install **before** having a coding agent implement §7 — prevents hallucinated request fields |

### 1.2 Response shape (exact)

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "opponent_archetype": {
      "type": "choice",
      "choice": "lag",
      "probabilities": { "nit": 0.02, "fit_or_fold": 0.05, "station": 0.08,
                          "reg": 0.20, "lag": 0.55, "maniac": 0.08, "unknown": 0.02 },
      "confidence": 0.61
    },
    "leverage_spot": { "type": "noul", "noul": 0.81 },
    "icm_pressure": { "type": "score", "score": 2.3,
      "legend": { "0": "...", "1": "...", "2": "...", "3": "..." },
      "probabilities": { "0": 0.0, "1": 0.1, "2": 0.6, "3": 0.3 },
      "confidence": 0.74 }
  },
  "usage": { "input_tokens": 1650, "output_tokens": 64 }
}
```

(The numbers are illustrative; the **shape is exact** per docs.typesafe.ai.)

### 1.3 Vendor claims vs. what to trust

Trust, because it is structural: typed outputs can't be malformed (0% *type*-error rate is by
construction, not empirics); probabilities arrive as full distributions; parallel fan-out batching
is real and measured by third parties.

Treat as marketing ceilings: "193.6× faster / 444.6× cheaper / never hallucinates."
Independent write-ups note the multiples come from TypeSafe's own workflow evals at the high end;
"zero hallucination" means *zero schema violations* — the model can still be **confidently wrong**.
Any single answer can be wrong; calibration is a distributional property. Therefore §9's
shadow-first rollout and Brier-score calibration harness are not optional.

Documented weaknesses ("jaggedness" of `jev-1.13`) that map directly onto poker anti-patterns —
full list in §8. Headlines: no arithmetic, no counting, no date math, no indirection, literal
reading of criteria, Scores are for thresholding/ranking not magnitudes, state pollution (context
rot) degrades accuracy, adversarial text inside state can steer answers.

### 1.4 Prior art directly relevant to you

- madewithjev.com catalogs **"Games and real time: 51 builds"** — Tetris move chooser, driving
  simulator (structured observations → accelerate/brake/turn), a Doom bot on structured game state.
  Poker is the same shape: structured observations → typed decision support.
- A **"Jev Test Bench — Texas Hold'em Edition"** (GitHub, served via Vercel AI Gateway) already
  exists as a community harness; useful to crib request patterns from, and a sign the idea is sane.
- OpenJEV (openjev.sh) — open playground/wrapper for classify/route/score/rank; handy to prototype
  question wording before wiring Rust.

---

## 2. Doctrine — where Jev sits in pkr-sota

Three hard rules. Every task card in §4 and §7 obeys them.

**R1 — Never in the CFR hot path.** Your trainer does 15.6K it/s @1 thread (measured in
`docs/status.md`). A 100 ms network call would make that 10 it/s. Jev is called (a) at
**play time** (one call per real decision), (b) **offline** (hand triage, labeling), never inside
`traversal.rs`/`dcfr.rs` loops.

**R2 — Engine is self-sufficient offline.** If `JEV_ENABLED=0`, on API error, timeout (>400 ms),
or low confidence, every path falls back to the exact same behavior the engine has today
(blueprint action, pseudo-harmonic translation, no re-solve). Jev can only *upgrade* decisions,
never block them. There is one kill switch and one fallback default per feature.

**R3 — Jev classifies; Rust computes.** Equities, pot odds, M, ICM values, VPIP/PFR/3bet stats,
outs, combo counts: computed in Rust and passed into state as **facts**. Jev's questions are about
**judgment on those facts** ("does `opp_stats` suggest a capped range given `line_history`?"),
never arithmetic ("never ask: what is 3/4 of the pot").

```
                       ┌────────────────────────────────────────────────┐
                       │                PLAY TIME (online)              │
 hole cards + action   │                                                │
 ────────────────────► │  pkr-runtime (mmap FMph blueprint)             │
                       │        │ top-k candidate actions + probs       │
                       │        ▼                                       │
                       │  pkr-meta::build_state()   ◄─ pkr-core facts   │
                       │  (regime, stacks-bb, M, stats, texture labels) │
                       │        │ one call, ~13 questions (fan-out)     │
                       │        ▼                                       │
                       │  Jev (network, 100–300 ms)  ── on error/low    │
                       │        │ answers + confidence    conf: fallback│
                       │        ▼                                       │
                       │  pkr-meta::apply(): blueprint router /         │
                       │  exploit preset / size extension / re-solve    │
                       │  gate (riversolve.rs) / translation override   │
                       │        ▼                                       │
                       │  final action → table                          │
                       └────────────────────────────────────────────────┘

                       ┌────────────────────────────────────────────────┐
                       │              TRAIN TIME (offline, M1)          │
                       │  hand histories / self-play logs               │
                       │        ▼                                       │
                       │  pkr-meta::triage (batch, 1 call/hand)         │
                       │  leak Nouls × severity Scores                  │
                       │        ▼                                       │
                       │  curriculum weights → DCFR discount tweaks     │
                       │  archetype-conditioned blueprint targets       │
                       │  calibration harness (Brier) in pkr-exploit    │
                       └────────────────────────────────────────────────┘
```

**Why this wins tournaments specifically.** A blueprint approximates the *Nash equilibrium of
your abstraction*; tournament equity comes from (a) playing the right **blueprint variant** for
the right tournament state (stage, stack depth, ICM pressure), (b) **deviating from it** against
the actual population, and (c) handling the **long tail of off-tree spots** your 6-bucket action
abstraction can't represent. Those three are judgment problems with small states and typed
answers — exactly Jev's shape, and exactly where pure GTO engines leave EV on the table.

---

## 3. Codebase map for this integration

**New crate (one):** `crates/pkr-meta/` — everything Jev lives here:
`client.rs` (REST), `types.rs` (answers), `questions.rs` (packs), `state.rs` (state builder),
`advisor.rs` (trait + gates), `router.rs` (M1), `presets.rs` + `stats.rs` (M2/M8/M9),
`size_extend.rs` (M3), `gates.rs` (M5), `icm.rs` (M7), `triage.rs` (M10/M11), `cache.rs`.

**Touched files (thin hooks only):**

| Crate / file | Change |
|---|---|
| `binaries/pkr-trainer/src/main.rs` | play mode: call advisor after blueprint lookup; add `--meta {off,shadow,live}` flag; consume curriculum weights file for sampling |
| `crates/pkr-runtime/src/lookup.rs` | **no change needed** — `SolverHandle::lookup(infoset_hash) -> Option<SotaAdvice>` already returns the CDF over buckets; `pkr-meta` converts CDF→PMF and derives top-k + margin |
| `crates/pkr-export/src/translate.rs` | **no rewrite needed** — `compute_translation(lower, upper, actual, reach_lower, reach_upper) -> (u8,u8)` already exists and is fully tested; M4 only wires intent → anchor selection around it |
| `crates/pkr-abstraction/src/lib.rs` | add `pub fn cluster_id(&self, hole, board) -> u64` wrapper (reuses private `flat_index_*` + `nearest_centroid` + flop-bucket table) so the state builder can label buckets without re-deriving them |
| `crates/pkr-core/src/state.rs` | add `pub fn action_bucket(fraction_of_pot, is_allin) -> u8` — extracts the constant table from `abstract_action_index_static` so `pkr-meta` doesn't duplicate it |
| `crates/pkr-cfr/src/riversolve.rs` | already a library fn; M5 gate wraps it at the call site (no change inside) |
| `crates/pkr-exploit/src/lib.rs` | add `brier.rs` calibration metrics (M12) |

**Never touched:** `crates/pkr-cfr/src/{dcfr.rs,traversal.rs,table.rs}` — the M1 training loop
stays bit-identical; Jev has no dependency into it (R1).

---

## 4. Meta-improvement catalog (13 cards)

Format per card: **What / Why it wins tournaments / Jev question(s) / Integration / Fallback / Risk**.
Risk tiers: 🟢 low (pure addition, hard fallback), 🟡 medium (changes chosen action when active),
🔴 ambitious (needs its own validation before live use).
Cross-reference: T-numbers below refer to task cards in the companion playbook
(`pkr-sota-M1-playbook.md` §4) — e.g. T2.1 action-abstraction fixes.

### Tier A — Runtime meta-layer (one Jev call per real decision)

**M1 — Tournament-stage blueprint router** 🟢
- *What:* You will (per playbook T2.x) own several trained blueprints. A `Choice` over a blueprint
  registry picks which one drives this decision: early/mid/deep-stack game, bubble, ITM,
  final table, and (later) hero-stack bucket.
- *Why:* Tournament EV is stage-dependent. One cash-style blueprint over-shoves ITM and
  under-pressures bubbles. Routing 1 file swap per stage is the single biggest meta gain per line
  of code.
- *Jev:* `stage_regime` (Choice: `early|middling|approaching_bubble|on_bubble|itm|final_table`)
  + `icm_pressure` (Score 0–3, levels describe situations, §6 P2).
- *Integration:* registry map `stage → .fmph path` loaded by `pkr-runtime` (`crates/pkr-runtime/src/mmap.rs`);
  router in new `crates/pkr-meta/src/router.rs`. Stack/M/bblvl/players-left are state facts from
  the client feed; ICM value itself is computed locally (M11) if you enable it.
- *Fallback:* stage `unknown` or confidence < 0.6 → default blueprint (today's file).
- *Depends on:* you having ≥2 blueprints (playbook T2/T3 outputs). Ship the router code before the files exist.

**M2 — Opponent archetype classifier → exploit preset** 🟡
- *What:* Local stats window (last N hands per opponent: VPIP, PFR, 3bet, WTSD, fold-to-cbet,
  river-fold — computed in Rust) goes into state; a `Choice` returns the archetype
  (`nit|fit_or_fold|station|reg|lag|maniac|unknown`), plus Nouls for the *primary leak*
  (`overfolds_river`, `calls_down_too_wide`, `overbluffs`). Your code maps archetype+leaks to one
  of K **exploit presets** (pre-trained counter-blueprints, or mix-shift parameters applied on top
  of the GTO blueprint: e.g. +bluff frequency vs `overfolds_river`).
- *Why:* Against a real tournament population the money is in punishing fit-or-folds and stations,
  not in unexploitable play. This is the classic "GTO baseline + exploitative overlay" architecture
  (GTO Wizard-style population overlays, RL-CFR's exploitation layer), implemented without
  retraining anything.
- *Jev:* 4 questions in the runtime pack (P1): `opponent_archetype` (Choice),
  `primary_leak` (Choice incl. `balanced_unknown`), `opp_range_capped` (Noul), `opp_tilting` (Noul).
- *Integration:* new `crates/pkr-meta/src/stats.rs` (compute stats from hand feed) +
  `presets.rs` (archetype → preset parameters). Blueprint mixing happens at action-selection time
  in `pkr-runtime` lookup wrapper — the `.fmph` files are untouched.
- *Fallback:* archetype confidence < 0.55 or < 15 hands of stats → pure blueprint.
- *Note:* start with **mix-shift presets** (small, auditable weight tweaks you can A/B in
  `pkr-testgames`) before training counter-blueprints.

**M3 — Action-abstraction extender (beyond the 6 buckets)** 🟡
- *What:* Your action abstraction has 6 buckets with known defects (empty bucket 2; 1.0x/2.0x
  collision in bucket 4 — see playbook §1). Playbook T2.1 fixes the defects; **M3 lets Jev widen
  the choice set at play time**: when the blueprint's top action has low margin (you expose
  top-k probabilities from the regret table), ask Jev a `Choice` over an extended size ladder
  (e.g. `33|50|75|100|125|150|allin`) *as a parallel suggestion channel*, and route the chosen
  size through the existing translation path (pseudo-harmonic, playbook T2.3 / `translate.rs`).
- *Why:* Fixed 6 sizes are a measurable EV leak vs solvers using 8–12 sizes per node
  (RL-CFR 2024; Fucus bet-sizing work). This buys granularity where it matters without retraining.
- *Jev:* `size_intent` (Choice over ladder, criteria describe when each size is used) +
  `board_texture` (Score, P2).
- *Integration:* only fires when `pkr-runtime` reports candidate-margin < τ (computed from the
  `SotaAdvice` CDF→PMF conversion, JT1); the chosen size snaps to the ladder `GameState::legal_actions`
  already bets on (fractions `[0.5, 1.0, 2.0]` of pot + all-in — see
  `pkr-core/src/state.rs`), result merged with blueprint mix (e.g. 70% blueprint / 30% Jev size,
  tunable constant), all in `crates/pkr-meta/src/size_extend.rs`.
- *Fallback:* margin ≥ τ, timeout, or confidence < 0.6 → blueprint sizes only.

**M4 — Off-tree action interpreter** 🟡
- *What:* When an opponent bets a size outside your abstraction (the long tail), the engine
  currently maps it via the (unwired) pseudo-harmonic translation. Add a Jev `Choice` over
  translation *intent classes* (`thin_value|polar_value|protection|bluff|blocker_probe|ambivalent`)
  given the state facts; intent selects the translation anchor (map polar → nearest overbet
  anchor, thin value → nearest small anchor, etc.).
- *Why:* Bad off-tree translation is a direct equity leak every time an opponent makes an
  unusual-size bet — which is exactly what weak tournament players do.
- *Integration:* the translation math already exists —
  `crates/pkr-export/src/translate.rs :: compute_translation(lower, upper, actual, reach_lower,
  reach_upper) -> (u8, u8)` (pseudo-harmonic, Ganzfried & Sandholm 2013) — it is fully unit-tested
  but unwired. M4 = intent → anchor selection (which `lower`/`upper` pair to feed it) wrapped at
  runtime lookup (`pkr-runtime/src/lookup.rs` consumer side), behind the same confidence gate.
- *Fallback:* intent confidence < 0.55 → current translation function unchanged.

**M5 — High-leverage re-solve gate** 🟡
- *What:* You will have a river re-solver (`crates/pkr-cfr/src/riversolve.rs`, playbook T3.1).
  It is too slow to run every hand on an M1. A `Noul` gate (`leverage_spot`: describes pot/stack
  ratio situation, pay-jump proximity, multiway pressure) + a local margin check decide when to
  spend 2–10 s of local re-solve.
- *Why:* Compute rationing *is* the M1 strategy. Spending it only on leverage spots maximizes
  EV per CPU-second.
- *Integration:* `crates/pkr-meta/src/gates.rs`; gate opens the existing riversolve path in
  `pkr-trainer`'s play mode.
- *Fallback:* Noul ≤ 0.7 → blueprint only (status quo).

**M6 — Confidence-gated composite (the safety envelope)** 🟢
- *What:* Not a feature but the envelope for M1–M5: every runtime Jev application respects
  per-question thresholds (§6.3), and the whole pack is one API call (speculative fan-out —
  13 questions cost the same latency as 1). Questions whose branch isn't taken still get used for
  logging/calibration (§9).
- *Fallback:* is the feature.

### Tier B — Tournament / meta-game edge

**M7 — ICM regime classifier + local ICM engine** 🟡
- *What:* Two parts. (a) Local: a small exact ICM calculator over the payout table and current
  stacks (new `crates/pkr-meta/src/icm.rs`, ~120 lines, pure math — NOT a Jev job). It emits
  `$EV` pressure facts (bubble distance, pay-jump ratio, stack-rank deltas). (b) Jev: `Score`
  `icm_pressure` on described situations + `Noul` `pay_jump_squeeze` given those facts.
- *Why:* Postflop grit, shove/fold looseness, and calling ranges all shift with ICM pressure.
  The classifier turns your single blueprint into stage-aware behavior without training
  ICM-specific blueprints (later, Tier 🔴, you can condition training on it).
- *Fallback:* pressure score ≤ 1.0 → cash-style behavior (status quo).

**M8 — Table-image tracker** 🟡
- *What:* Rolling local log of hero's own recent line distribution + showdown reveals (did we
  show a bluff? got caught value-betting thin?). State includes a compact summary string built in
  Rust ("last 3 sd: cbet-folded, barreled-bluff river, folded river x2"); Jev `Noul`s:
  `hero_image_aggressive`, `hero_image_read_as_station`, `opponent_adapting`.
- *Why:* Exploit presets (M2) assumed static opponents. Image facts let presets *de-escalate*
  bluffs after two failed river raises against the same villain — the human "meta" layer.
- *Fallback:* no image summary → skip Nouls (they're additive weight tweaks).

**M9 — Regime-shift detector** 🟢
- *What:* Every orbit (or N hands), recompute per-opponent stats deltas locally; one Jev call
  with a `Noul` per opponent (`tendency_shifted`, criteria: compare `stats_prev` vs `stats_now`
  windows) + `Choice` `shift_direction`.
- *Why:* Tournament tables churn; final-table opponents adjust. Catches the villain who
  woke up, before your static preset bleeds for an orbit.
- *Fallback:* none needed — output only modulates preset selection confidence.

### Tier C — Training-time meta-layer (offline; Jev as cheap labeler)

**M10 — Hand-history leak triage → training curriculum** 🟢
- *What:* Batch-label every hand your bot plays (self-play logs or imported HH): 8 `Noul`s per
  hand (one per leak class: `overfolded_river`, `underbluffed_turn`, `called_too_wide_preflop`,
  `overplayed_marginal`, `missed_thin_value`, `overfolded_vs_barrel`, `wrong_size_vs_texture`,
  `icm_error_flavor`) + 1 `Score` `leak_severity` (0–3). Aggregate → leak histogram → reweight
  the *sampling distribution* of your MCCFR traversal (which subtrees get extra iterations) —
  a curriculum on top of playbook T1.x.
- *Why:* 3.34B iters/day is only useful if spent where the bot leaks. This is a data-driven
  alternative to uniform iteration budget.
- *Cost:* ~1.2k tokens/hand → **~$50 per 1M hands**, an evening of runtime at 1,200 req/min.
- *Integration:* `crates/pkr-meta/src/triage.rs` (batch, retry, cache) → emits a weights file
  consumed by a new flag in `binaries/pkr-trainer` sampling code.

**M11 — Population profiling → counter-blueprint targets** 🔴
- *What:* Same triage machinery, pointed at *imported opponent hands* (with consent/where legal):
  cluster the population into archetype frequencies; generate archetype-conditioned blueprint
  targets for the top 3 archetypes (train K counter-blueprints vs the archetype's range
  distribution, per playbook T3 pipeline).
- *Why:* This is how commercial "populationexploit" products work. It is the biggest long-run
  edge in this document and the most expensive: requires the T2/T3 training pipeline to be solid
  first.

**M12 — Calibration harness (Brier + agreement)** 🟢
- *What:* For every runtime and triage call, log `{model_version, question_id, probabilities,
  confidence, outcome}` (outcome = did the opponent actually fold? did the re-solve agree with
  the blueprint? did the hand land in the predicted regime?). Nightly job computes Brier/log-loss
  and calibration curves per question in `crates/pkr-exploit` (it already has metric plumbing).
- *Why:* RLCD promises calibration; verify it *on your distribution* before trusting thresholds.
  Also detects drift when TypeSafe ships `jev-1.14`.
- *Acceptance:* each shipped question needs Brier ≤ 0.20 on ≥ 5k logged outcomes, else it
  auto-downgrades to fallback behavior (threshold tightening, §6.3).

### Tier D — Dev workflow

**M13 — TypeSafe agent skill + question review loop** 🟢
- *What:* Before implementing §7 with a coding agent, install TypeSafe's official agent skill
  (`npx skills add typesafe-ai/skills --skill typesafe-ai`). Keep **all** questions and thresholds
  in one file (`crates/pkr-meta/src/questions.rs` + `questions.json` mirror) so review, diffing,
  and replay tests (§9) touch one place. Add a CI `Noul`-based smoke check: the mock-server test
  set (JT0) replays a frozen state and asserts answers parse and confidences are within logged
  historical bands.
- *Why:* TypeSafe's own guidance: agents trained on LLM APIs write one-question-per-call and
  invent fields; the skill + single-file rule prevents both.

---


## 5. The state you give Jev — with field-by-field provenance

Every field below is anchored to a concrete symbol in your codebase. Three statuses:
**EXISTS** = the value is already produced by a public function/struct today;
**ADAPTER** = thin glue in `pkr-meta` over existing public data (a few lines each);
**NEW** = genuinely new computation, small and testable (never CFR math).

> ⚠️ **Heads-up note:** `pkr_core::state::GameState` is a 2-player HU structure
> (`stacks: [f32; 2]`, `hole: [[u8;2];2]`, `actor`/`dealer: usize`, actor flips via `1 - actor`).
> The whole meta layer is therefore specified for **heads-up play**: `villain` is *the* single
> opponent. Multiway tournaments still work — the engine plays HU subgames vs one live villain —
> but the state carries exactly one villain. Do not invent multiway fields the engine can't fill.

### 5.1 Filtering doctrine (from docs: context rot is real)

1. **Facts, not math.** Numbers are pre-computed in Rust from `GameState` and the tournament
   feed: `pot_bb`, `spr`, stacks in bb, M, stats, EHS. Jev's questions are judgments *on those
   facts*, never arithmetic ("never ask: what is 3/4 of the pot").
2. **Filter to what the questions reference.** Only fields the §6 pack actually cites.
3. **Stable enum names.** `state_version` gates replay tests (§9); bump on any field rename.
4. **English text only** in labels/criteria (Jev is English-first per docs).
5. **No adversarial content.** Only engine-produced fields; never opponent chat.

### 5.2 Runtime decision state v2 (exact JSON the Rust builder emits)

```json
{
  "state_version": "2.0",
  "format": "nlhe_hu_mtt",
  "street": "river",
  "board_flags": { "paired": true, "two_tone": true, "straight_possible": false,
                    "flush_draw_possible": false,
                    "texture_label": "paired two-tone, no made straights, no flush draw" },
  "hand_summary": { "hero_hand_rank": 2548, "hero_made_label": "two pair",
                     "hero_equity_estimate": 0.62, "draw_label": "none",
                     "preflop_bucket_hint": 214 },
  "pot": { "pot_bb": 18.5, "spr": 1.6, "pot_fraction_to_call": 0.33,
           "facing": "river, villain bet 33% pot after check-call, check-call" },
  "stacks": { "hero_stack_bb": 29.6, "villain_stack_bb": 31.0, "eff_stack_bb": 31.0 },
  "tournament": { "players_left": 24, "paid_places": 18, "avg_stack_bb": 26.0,
                   "hero_rank_by_stack": 9, "bb_level": 12, "hands_to_next_level": 6,
                   "payout_top3_share": "55/25/12 pct", "bubble_distance_places": 6,
                   "pay_jump_next": "min-cash to 3x min-cash" },
  "villain": { "hands_seen": 41,
               "stats": { "vpip": 0.31, "pfr": 0.22, "threebet": 0.06,
                           "fold_to_river_bet": 0.41, "wtsd": 0.27 },
               "line_history": "flop: check-call 0.5 pot; turn: check-call 1.0 pot",
               "recent_showdowns": "lost showdown with second pair, made one big river call" },
  "hero_image": { "showdowns_last_10": "one shown bluff on turn, two thin value shows",
                   "aggression_index": 0.62 },
  "engine": { "bucket_ids": { "hand_bucket": 12, "board_bucket": 3, "flop_bucket": 7 },
               "blueprint_top_actions": [ { "bucket": "half_pot", "p": 0.41 },
                                          { "bucket": "check_call", "p": 0.38 },
                                          { "bucket": "pot", "p": 0.12 } ],
               "candidate_margin": 0.03,
               "legal_size_ladder_bb": [ 9.3, 18.5, 37.0, "allin 29.6" ],
               "off_tree_opponent_action": "villain bet 0.33 pot (not an abstract bucket)" }
}
```

Typical serialized size: ~1.3–1.8k tokens → runtime call ≈ **$0.00007–0.00010**.

### 5.3 Provenance map — where every field comes from

Legend: **EXISTS** public symbol today · **ADAPTER** glue over public data in `pkr-meta` ·
**NEW** new small computation. Path notation `file.rs :: item`.

| JSON field (§5.2) | Source in the engine | Status |
|---|---|---|
| `street` | `pkr-core/src/state.rs :: GameState.street` (`Street::{Preflop,Flop,Turn,River}`) | EXISTS + ADAPTER (lowercase serde) |
| `board_flags.*` (booleans) | computed from `GameState.board[..board_len]` (raw `u8` cards 0..51; decode rank = `c % 13`, suit = `c / 13` — matches `pkr-core/src/deck.rs :: Deck::new` ordering: suits outer, ranks inner, `Two=0..Ace=12`) | NEW (`pkr-meta/src/texture.rs`, pure fns + unit tests on fixed boards) |
| `board_flags.texture_label` | same inputs, enum → `&'static str` mapping | NEW (same file; no generation) |
| `hand_summary.hero_hand_rank` | `pkr-eval` `TableEvaluator::evaluate_hand(&hole, &board)` (trait `pkr-contracts/src/lib.rs :: Evaluator`; trainer already builds it — `binaries/pkr-trainer/src/main.rs :: TableEvaluator::new(&rank_table)`) | EXISTS |
| `hand_summary.hero_made_label` | derive from `hero_hand_rank` via thresholds (pair/two-pair/trips/straight/flush/…) | NEW (`pkr-meta/src/handlabel.rs`; golden tests vs `TableEvaluator` on fixed hands) |
| `hand_summary.hero_equity_estimate` | `pkr-abstraction/src/ehs.rs :: calculate_ehs(hole, board, evaluator) -> (f32, f32)` — already `pub`, re-exported as `pkr_abstraction::calculate_ehs`. Use the first tuple element (EHS = win prob vs random hand). ~1,000 MC samples (`EHS_SAMPLES` env) — ms-scale, play-time only (R1) | EXISTS |
| `hand_summary.preflop_bucket_hint` | the EHS² cluster id. Today the cluster id is only embedded inside `get_infoset_hash`; add a 5-line public wrapper `pub fn cluster_id(&self, hole, board) -> u64` in `pkr-abstraction/src/lib.rs` reusing `flat_index_*` + `nearest_centroid` | NEW (tiny wrapper over EXISTS internals) |
| `hand_summary.draw_label` | hole+board analysis (4-to-flush, open-ended, gutshot) | NEW (`texture.rs`) |
| `pot.pot_bb` | `GameState.pot` ÷ `bb` (bb comes from `TournamentContext`, below) | ADAPTER |
| `pot.spr` | `GameState.stacks[GameState.actor] ÷ GameState.pot` | ADAPTER |
| `pot.pot_fraction_to_call` | `GameState.bet_to_call()` (EXISTS) ÷ `(pot + bet_to_call)` | EXISTS fn + ADAPTER |
| `pot.facing` | formatted from `GameState.history`, `actions_this_street`, and `GameState.history_signature()` (EXISTS: raises count + aggressor flag packed in u32) | ADAPTER |
| `stacks.*_bb` | `GameState.stacks` ÷ bb; `eff_stack_bb = min(hero_stack + hero_street_bet, villain_stack + villain_street_bet)` using `street_bets` | ADAPTER |
| `tournament.*` (all) | **Not in the engine.** `GameState` has no tournament context. NEW struct `TournamentContext { players_left, paid_places, avg_stack_bb, hero_rank_by_stack, bb_level, hands_to_next_level, payout_table_facts, bubble_distance_places, pay_jump_next }` in `pkr-meta/src/context.rs`, filled by the platform/feed adapter once per hand. ICM numbers (when enabled, M7) come from `pkr-meta/src/icm.rs` and are passed as *facts* | NEW (struct + adapter; Jev never computes it) |
| `villain.hands_seen`, `villain.stats.*` | NEW sliding-window counters in `pkr-meta/src/stats.rs` (JT4). Feed = observed `Action { player, kind }` stream (`ActionKind::{Fold,Check,Call,Bet}`) from play mode | NEW |
| `villain.line_history` | last K actions formatted from `GameState.abstract_history` (bucket ids 0–5 via `abstract_action_index_static` semantics: 0 fold, 1 check/call, 2 <½, 3 ½–1, 4 ≥1, 5 all-in) + sizes from `history` | ADAPTER |
| `villain.recent_showdowns` | play-feed showdown records (cards revealed + winner) — NEW ring buffer in `stats.rs` | NEW |
| `hero_image.*` | NEW `ImageTracker` (JT4) over hero's own shown-down lines | NEW |
| `engine.bucket_ids.hand_bucket / board_bucket` | river: `hand_rank >> 6` and board-bucket mix-in — same math as `pkr-abstraction/src/lib.rs :: get_infoset_hash` (lines already public in the fn); expose via the same `cluster_id`-style wrapper | NEW wrapper (EXISTS math) |
| `engine.bucket_ids.flop_bucket` | flop-bucket table loaded via `load_flop_buckets`; id = `table[combinadic index of top-3 board cards]` (private `flop_bucket()` today — include in the wrapper PR) | NEW wrapper |
| `engine.blueprint_top_actions` | `KMeansAbstraction::get_infoset_hash(hole, board, abstract_history, street)` → `pkr-runtime/src/lookup.rs :: SolverHandle::lookup` (trait `BlueprintProvider` in `pkr-contracts`) → `SotaAdvice { cdf_probabilities: [u8;16], len }` → **CDF→PMF by adjacent differences** → top-3 with bucket names | EXISTS + ADAPTER |
| `engine.candidate_margin` | `pmf[0] − pmf[1]` of the same PMF (drives M3's trigger) | ADAPTER |
| `engine.legal_size_ladder_bb` | the exact ladder `GameState::legal_actions` bets on — fractions `[0.5, 1.0, 2.0]` of `pot` + all-in (`stacks[actor]`), clamped by stack (`legal_actions_into`); emit in bb | EXISTS + ADAPTER |
| `engine.off_tree_opponent_action` | villain's last `Action` from `GameState.history[history_len-1]`; for `Bet(total)`: fraction = `total ÷ pot`; if it matches none of {0.5, 1.0, 2.0, all-in} ⇒ off-tree. Same math as `pkr-core/src/state.rs :: abstract_action_index_static` — add `pub fn action_bucket(fraction) -> u8` in `pkr-core` to avoid duplicating the constant table | ADAPTER + tiny pkr-core helper |

**Provenance summary:** ~60% of the state is EXISTS/ADAPTER over symbols already in the dump
(`GameState`, `calculate_ehs`, `get_infoset_hash`+`SolverHandle`+`SotaAdvice`,
`compute_translation`, `history_signature`, `legal_actions`). Only three groups are genuinely
NEW: tournament context, opponent stats/image windows, and board/hand label fns — all small,
pure, and unit-tested (JT1/JT4/JT5).

### 5.4 Offline hand-record state (triage / M10)

Same schema plus a timeline built by replaying `GameState.history[0..history_len]`
(each `Action { player, kind }` → `{street, actor, action, size_bb}` with street changes at
`advance_street_in_place` boundaries) and an `outcome` block from `total_invested` +
showdown reveal (`terminal_payoff` semantics: rank comparison via the same `Evaluator`).
Machine-formatted labels, not prose; ≤ 6k tokens/hand.

### 5.5 What never goes into state

Opponent chat; raw card strings where a bucket label exists; full payout tables (send the 2–3
relevant facts); anything requiring date/time or counting math; multiway fields the HU engine
cannot fill.

---

## 6. Question packs — ready to paste (the "questions to give Jev")

All packs use the exact request schema from docs.typesafe.ai. Rules honored throughout:
one judgment per question; instructions reference backticked state paths; criteria **describe
situations, not degrees**; every Choice gets an `other/unknown` escape; no negatives-as-true;
no math asked. Store as constants in `crates/pkr-meta/src/questions.rs` **and** as a mirror
`questions.json` (M13).

### 6.1 P1 — `RUNTIME_DECISION_PACK` (one call per real decision; speculative fan-out)

> HU note: every state path below resolves against the §5.2 v2 schema — one villain (the single
> live opponent), `tournament.*` facts from the feed-side `TournamentContext` (§5.3), and
> `engine.*` facts derived from `SotaAdvice`/`GameState` provenance rows.

```json
{
  "stage_regime": { "type": "choice",
    "instructions": "Given `tournament` and `stacks` in the state, which tournament phase is this hand in?",
    "criteria": {
      "early": "Players_left is high relative to paid_places, stacks deep (eff_stack_bb 25+), payouts far away",
      "middling": "Approaching the money but bubble_distance_places is 12+ and stacks still playable",
      "approaching_bubble": "bubble_distance_places is 12 or fewer and pay_jump_next describes min-cash nearby",
      "on_bubble": "bubble_distance_places is 3 or fewer, or next elimination decides the min-cash",
      "itm": "In the money but not final_table: pay jumps are modest multiples of min-cash",
      "final_table": "players_left is 9 or fewer",
      "other": "State is inconsistent or from a non-tournament format" } },

  "icm_pressure": { "type": "score",
    "instructions": "How much should prize-pool pressure distort ordinary chip-EV strategy for the hero in this hand, judging `tournament` and `stacks`?",
    "criteria": [
      "Payouts effectively irrelevant; chip-EV is fine",
      "Mild pressure: modest pay jumps, comfortable stack, no immediate bubble",
      "Real pressure: near the bubble or a large pay jump with a below-average stack",
      "Severe pressure: bubble hand, tiny stack, or pay jump where losing this pot changes real money tiers" ] },

  "villain_archetype": { "type": "choice",
    "instructions": "Classify the opponent described by `villain.stats` and `villain.line_history`.",
    "criteria": {
      "nit": "Plays very few hands: vpip under 0.18 with pfr close to vpip, rarely calls down",
      "fit_or_fold": "Enters often but continues rarely: moderate vpip, low pfr, high fold_to_river_bet",
      "station": "Calls too much: high wtsd, low fold_to_river_bet, rarely raises without nuts",
      "reg": "Balanced aggressive winner profile: vpip 0.20-0.28, pfr 0.16-0.24, sensible fold_to_river_bet",
      "lag": "Plays a lot and aggressively: vpip above 0.32 with pfr above 0.25, frequent position raises",
      "maniac": "Hyper-aggressive: very high vpip and pfr, frequent big raises, low fold frequencies",
      "unknown": "Fewer than 15 hands_seen or stats inconsistent" } },

  "villain_primary_leak": { "type": "choice",
    "instructions": "Which single exploitable tendency dominates `villain.stats` and `villain.line_history`?",
    "criteria": {
      "overfolds_river": "fold_to_river_bet is 0.55 or higher",
      "calls_down_too_wide": "fold_to_river_bet very low and wtsd above 0.30",
      "overbluffs": "line_history shows repeated big bets that lost at showdown",
      "overfolds_vs_3bet": "preflop pressure makes them release: implied by low threebet with high fold tendencies",
      "no_showdown_guts": "Checks back strong ranges and gives up rivers, seen in line_history",
      "balanced_unknown": "Stats near equilibrium frequencies or unknown" } },

  "villain_range_capped": { "type": "noul",
    "instructions": "Does `villain.line_history` plus `facing` suggest the opponent's continuing range is capped (strong value hands unlikely) on this street?",
    "criteria": { "true": "Line is passive: check-called smaller bets, no raise, consistent with drawing or medium-strength holdings",
                   "false": "Line includes raises, overbets, or limp-then-jam patterns consistent with strong value" } },

  "villain_tilting": { "type": "noul",
    "instructions": "Do `villain.recent_showdowns` and `villain.line_history` indicate the opponent is on tilt or stuck and playing agitated?",
    "criteria": { "true": "Recent lost showdowns followed by sudden over-aggression, unusual sizes, or erratic lines",
                   "false": "Line sizes and frequencies consistent with their baseline stats" } },

  "board_texture_class": { "type": "score",
    "instructions": "How dynamic and connected is the board described by `board_flags`?",
    "criteria": [
      "Static dry: `paired` false, `two_tone` false, `straight_possible` false",
      "Mildly dynamic: one of two_tone or straight_possible, no pairing",
      "Dynamic: two_tone with straight_possible, or `paired` with draws live",
      "Action-board: `paired` plus two_tone or straight_possible, many made-hand interactions" ] },

  "leverage_spot": { "type": "noul",
    "instructions": "Is this hand a high-leverage decision where a solver-grade re-solve is worth spending seconds of compute? Judge `pot`, `stacks`, `icm_pressure`, `stage_regime`.",
    "criteria": { "true": "Large fraction of effective stack in play at the decision, spr near or below 2, near a pay jump, or pot is a big fraction of hero_stack_bb",
                   "false": "Small relative pot, deep stacks, early stage with modest consequences" } },

  "hero_image_aggressive": { "type": "noul",
    "instructions": "Does `hero_image` suggest the table currently sees the hero as aggressive or bluff-prone?",
    "criteria": { "true": "Shown bluffs or high aggression_index above 0.55 in recent history",
                   "false": "Recent showdowns showed mostly value hands or the hero has been quiet" } },

  "opponent_adapting": { "type": "noul",
    "instructions": "Comparing `villain.stats` with `villain.line_history` and `villain.recent_showdowns`, has the opponent visibly shifted strategy within the sample?",
    "criteria": { "true": "Recent lines contradict their longer-run stats, e.g. a fit-or-fold profile suddenly barreling",
                   "false": "Recent lines match the stats window" } },

  "off_tree_intent": { "type": "choice",
    "instructions": "If `engine.off_tree_opponent_action` indicates a size outside the abstraction, what is the most likely intent of that bet? Otherwise answer off_tree_absent.",
    "criteria": {
      "thin_value": "Small-to-medium size from a player who shows down medium holdings; consistent with station tendencies",
      "polar_value": "Large size consistent with nutted or near-nut hands given the line_history",
      "protection": "Mid size with draws likely live on board_texture_class 2 or 3",
      "bluff": "Size inconsistent with any made-hand portion of a capped range",
      "blocker_probe": "Small size on boards where the bettor's folding range dominates",
      "ambivalent": "Cannot be placed with the given facts",
      "off_tree_absent": "The opponent action was already inside the abstraction" } },

  "size_intent": { "type": "choice",
    "instructions": "If the hero bets this street, which size best expresses the desired strategy given `hand_summary`, `board_texture_class`, `villain_primary_leak` and `pot`? Ignore current engine candidates.",
    "criteria": {
      "size_25": "Thin value or blocker bets vs capped calling ranges; fold equity unneeded",
      "size_50": "Standard c-bet width; polar ranges with medium protection needs",
      "size_75": "Strong value with some protection; vs stations slightly larger than standard",
      "size_100": "Polar overbet-ish pressure vs capped ranges on dynamic boards",
      "size_150": "Rare: exploit overfold tendencies with big sizing as a pure pressure line",
      "allin": "spr 1 or below, or leverage vs short stacks near pay jumps",
      "no_preference": "Check is fine; engine blueprint already handles it" } },

  "size_anomaly": { "type": "choice",
    "instructions": "Classify the opponent's bet size in `engine.off_tree_opponent_action` relative to `pot` and their `villain.stats`.",
    "criteria": {
      "standard": "Within 20 pct of the sizes implied by their other lines",
      "probe": "Small size into a checked pot after passive earlier streets",
      "overbet": "Well above pot-sized given their profile, suggesting either polar value or protection",
      "likely_misclick": "Absurdly small or large relative to pot and their baseline, near-certain input error",
      "other": "None of the above" } }
}
```

13 questions, ~600–800 tokens, one call, ~100–300 ms. Answers not relevant to the taken branch
are still logged for calibration (M12).

### 6.2 P2 — `FINAL_TABLE_PACK` (adds to P1 when `players_left ≤ 27`)

```json
{
  "pay_jump_squeeze": { "type": "noul",
    "instructions": "Do `tournament.pay_jump_next` and `tournament.bubble_distance_places` describe a situation where one elimination or one pot swings a large monetary tier for the hero?",
    "criteria": { "true": "Next pay jump is 2x or more the current tier and hero_rank_by_stack is within 4 places of it, or on_bubble",
                   "false": "Payouts currently change smoothly per place" } },

  "shove_fold_regime": { "type": "choice",
    "instructions": "Given `stacks.eff_stack_bb` and `pot`, should this decision be treated as a short-stack push/fold-style spot?",
    "criteria": {
      "push_fold": "eff_stack_bb is 10 or fewer, or spr below 1.5",
      "short_fight": "eff_stack_bb is 10 to 20 with a single raise pot",
      "normal": "Deeper than 20 bb with multi-street play viable" } }
}
```

### 6.3 P3 — `TRIAGE_PACK` (offline labeling, M10 — one call per recorded hand)

```json
{
  "leak_overfolded_river": { "type": "noul",
    "instructions": "In `actions_timeline`, did the hero face a river bet with a hand whose `hand_summary` suggests bluff-catcher strength, and fold while `outcome` shows the pot was won by a non-showdown or a weak reveal?",
    "criteria": { "true": "Hero folded the better hand or a clear bluff-catcher vs a size the timeline shows was frequently bluffed",
                   "false": "Fold was justified by the timeline" } },
  "leak_underbluffed_turn": { "type": "noul",
    "instructions": "Did the hero check back or give up on the turn with a draw or air component in `hand_summary` while the timeline shows the opponent released easily in similar spots?",
    "criteria": { "true": "A bet was clearly available and the opponent fold pattern supports it",
                   "false": "Check was reasonable" } },
  "leak_called_too_wide_preflop": { "type": "noul",
    "instructions": "Does `actions_timeline` show the hero cold-calling or over-limping with a `hero_bucket_preflop` in the bottom half of the range while stacks were shallow?",
    "criteria": { "true": "Call with shallow eff_stack_bb and dominated bucket",
                   "false": "Position/odds justified it" } },
  "leak_overplayed_marginal": { "type": "noul",
    "instructions": "Did the hero put in a large fraction of the stack with `hand_summary` labeling a marginal made hand against resistance shown in the timeline?",
    "criteria": { "true": "Multiple streets of heavy action with a one-pair-type hand",
                   "false": "Hand strength justified the aggression" } },
  "leak_missed_thin_value": { "type": "noul",
    "instructions": "Did the hero check the river with a `hand_summary` labeled strong-but-not-nut hand while the timeline shows an opponent capable of calling with worse?",
    "criteria": { "true": "Value bet was clearly available vs the villain profile",
                   "false": "Check was reasonable" } },
  "leak_overfolded_vs_barrel": { "type": "noul",
    "instructions": "Did the hero fold facing multiple barrels despite `hand_summary` showing a capped-but-live holding, while `pot` shows good odds?",
    "criteria": { "true": "Pot odds clearly favorable relative to the holding class",
                   "false": "Fold fine vs polar sizing" } },
  "leak_wrong_size_vs_texture": { "type": "noul",
    "instructions": "Do `board_flags` plus the hero's bet sizes in `actions_timeline` contradict each other (e.g. small bet into an action board with a strong hand)?",
    "criteria": { "true": "Size class mismatched texture and hand strength",
                   "false": "Sizing coherent" } },
  "leak_icm_error_flavor": { "type": "noul",
    "instructions": "Given `tournament` facts in the record, did the hero take a line that ignores prize pressure (e.g. heroically calling an all-in near the bubble with a medium stack)?",
    "criteria": { "true": "Line chip-EV correct but clearly ICM-negative given the facts",
                   "false": "Line consistent with the pressure" } },
  "leak_severity": { "type": "score",
    "instructions": "Overall, how costly was the play in this hand?",
    "criteria": [
      "Minor or no deviation from sound play",
      "Clear leak, small pot or recoverable",
      "Costly leak: meaningful stack fraction lost or won less",
      "Severe: tournament-defining pot misplayed" ] }
}
```

### 6.4 Thresholds — where code acts on answers

Defaults live next to the packs in `questions.rs`; change them only with §9 replay evidence.

| Answer | Gate | Behavior |
|---|---|---|
| `villain_archetype.confidence` | ≥ 0.70 | apply exploit preset (M2) |
| | 0.55 – 0.70 | blend preset 50% with blueprint mix |
| | < 0.55 | blueprint only |
| `villain_primary_leak` | ≥ 0.65 | preset modifier active |
| `stage_regime.confidence` | ≥ 0.60 | switch blueprint file (M1) |
| `leverage_spot.noul` | ≥ 0.75 **and** local margin check passes | run river re-solve (M5) |
| `off_tree_intent.confidence` | ≥ 0.60 | select translation anchor (M4) |
| `size_intent.confidence` | ≥ 0.70 **and** blueprint candidate margin < τ | allow extended size (M3) |
| any Noul used for gating | 0.50 floor | below the floor = model can't tell → always fallback (docs' review floor) |

**Version pinning:** call `model: "jev-1.13.0"` (pinned, not `jev-latest`) once thresholds are
tuned; log the response's `model` field on every call; re-run the §9 replay set before unpinning.

---

## 7. Rust task cards (dumb-agent executable)

Execute in order. Every card compiles and tests in isolation. Feature flags default OFF, so the
workspace builds and behaves exactly as today until §9 rollout flips them.

### JT0 — Scaffold `pkr-meta` + Jev client + typed answers  🟢

**Files:** `crates/pkr-meta/Cargo.toml`, `crates/pkr-meta/src/{lib.rs,client.rs,types.rs,questions.rs}`
**Depends on:** nothing.

`crates/pkr-meta/Cargo.toml`:
```toml
[package]
name = "pkr-meta"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ureq = { version = "2", features = ["json"] }

[dev-dependencies]
tempfile = "3"
```

`src/types.rs` (mirrors §1.2 exactly):
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerKind { Noul, Choice, Score }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Answer {
    #[serde(rename = "type")]
    pub kind: AnswerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noul: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default)]
    pub probabilities: std::collections::HashMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JevResponse {
    pub model: String,
    pub answers: std::collections::HashMap<String, Answer>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Usage { pub input_tokens: u64, pub output_tokens: u64 }
```

`src/client.rs`:
```rust
use crate::types::JevResponse;
use serde_json::Value;
use std::time::Duration;

/// ⚠️ VERIFY before first live call: confirm the exact endpoint + auth header
/// in https://docs.typesafe.ai/api (the Python/JS SDKs encode it; page any
/// docs URL as Markdown by appending `.md`). Kept as a constant so the fix
/// is one line.
pub const DEFAULT_ENDPOINT: &str = "https://api.typesafe.ai/v1/system-one";
pub const PINNED_MODEL: &str = "jev-1.13.0";
const TIMEOUT: Duration = Duration::from_millis(400);

pub struct JevClient {
    endpoint: String,
    api_key: String,
    agent: ureq::Agent,
}

pub struct JevRequest<'a> {
    pub state: &'a Value,
    pub questions: &'a Value,
}

impl JevClient {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("TYPESAFE_API_KEY").ok()?;
        Some(Self {
            endpoint: std::env::var("JEV_ENDPOINT").unwrap_or_else(|_| DEFAULT_ENDPOINT.into()),
            api_key,
            agent: ureq::AgentBuilder::new().timeout(TIMEOUT).build(),
        })
    }

    /// One call, all questions. One retry on 5xx; never panics — errors become None,
    /// and None ALWAYS means "fall back to blueprint" (doctrine R2).
    pub fn decide(&self, req: JevRequest<'_>) -> Option<JevResponse> {
        let body = serde_json::json!({
            "model": PINNED_MODEL,
            "state": req.state,
            "questions": req.questions,
        });
        for attempt in 0..2 {
            let resp = self.agent.post(&self.endpoint)
                .set("Authorization", &format!("Bearer {}", self.api_key))
                .send_json(&body);
            match resp {
                Ok(r) => return r.into_json::<JevResponse>().ok(),
                Err(ureq::Error::Status(code, _)) if (500..600).contains(&code) && attempt == 0 => continue,
                Err(_) => return None,
            }
        }
        None
    }
}
```

`src/questions.rs`: embed the packs from §6 with `include_str!("../questions.json")` and expose
`pub fn runtime_pack() -> Value`, `final_table_pack()`, `triage_pack()`; plus one Rust enum per
question id for exhaustive matching, and the §6.4 threshold table as constants.

`src/lib.rs`: module declarations + `pub mod prelude`.

**Verify:**
```bash
cargo test -p pkr-meta          # types deserialize a canned JevResponse fixture
cargo build --workspace         # nothing else changed
```
Test fixture: `src/testdata/response_fixture.json` with the exact shape of §1.2.

---

### JT1 — State builder (`state.rs`) with provenance-accurate inputs  🟢

**Depends on:** JT0. **Files:** `crates/pkr-meta/src/{state.rs,context.rs,texture.rs}`,
plus two tiny wrappers (below). Every value in the §5.2 JSON traces to a row of the §5.3 map.

**1) Tournament context (NEW — the only state group with no engine source):**
```rust
// crates/pkr-meta/src/context.rs
#[derive(Debug, Clone)]
pub struct TournamentContext {
    pub players_left: u16,
    pub paid_places: u16,
    pub avg_stack_bb: f32,
    pub hero_rank_by_stack: u8,
    pub bb_level: u16,
    pub hands_to_next_level: u16,
    pub payout_top3_share: String,   // "55/25/12 pct" — facts only
    pub bubble_distance_places: u16,
    pub pay_jump_next: String,
    pub bb: f32,                     // current big blind in chips
}
```
Filled by the platform/feed adapter once per hand. The engine never sees it (R1: nothing
tournament-specific enters `pkr-core`/`pkr-cfr`).

**2) DecisionInput + builder (state.rs):**
```rust
use pkr_core::state::{GameState, Street, ActionKind};
use pkr_contracts::SotaAdvice;
use serde_json::{json, Value};

pub struct StatsWindow { pub hands_seen: u32, pub vpip: f64, pub pfr: f64,
    pub threebet: f64, pub fold_to_river_bet: f64, pub wtsd: f64 }        // JT4 fills this
pub struct EngineFacts { pub advice: Option<SotaAdvice>, pub max_k: usize }

/// CDF -> PMF. SotaAdvice.cdf_probabilities is cumulative over abstract
/// buckets (0..len); adjacent differences give the pmf (u8-quantized).
pub fn pmf_from_advice(a: &SotaAdvice) -> Vec<f64> {
    let n = a.len as usize;
    let mut prev = 0u16;
    (0..n).map(|i| {
        let c = a.cdf_probabilities[i] as u16;
        let d = c.saturating_sub(prev);
        prev = c;
        d as f64
    }).collect()
}

pub fn build_decision_state(
    gs: &GameState,           // hero is seat 0 (set via set_hole_cards/actor conventions)
    tc: &TournamentContext,
    stats: &StatsWindow,
    eng: &EngineFacts,
    ehs: f32,                 // pkr_abstraction::calculate_ehs(...).0 — play-time only
    texture: &TextureLabels,  // texture.rs below
) -> Value {
    let bb = tc.bb.max(1.0);
    let to_call = gs.bet_to_call();
    let eff = (gs.stacks[0] + gs.street_bets[0]).min(gs.stacks[1] + gs.street_bets[1]) / bb;
    let mut top: Vec<(usize, f64)> = eng.advice.as_ref()
        .map(|a| pmf_from_advice(a).into_iter().enumerate().collect())
        .unwrap_or_default();
    top.sort_by(|x, y| y.1.total_cmp(&x.1));
    let top = top.into_iter().take(3);
    let bucket_name = ["fold", "check_call", "half_pot", "pot", "two_pot", "allin"];

    json!({
        "state_version": "2.0",
        "format": "nlhe_hu_mtt",
        "street": format!("{:?}", gs.street).to_lowercase(),
        "board_flags": texture.flags_json(),                    // texture.rs
        "hand_summary": { /* hero_hand_rank from TableEvaluator, labels from
                             handlabel.rs, hero_equity_estimate: ehs,
                             preflop_bucket_hint: wrapper below */ },
        "pot": { "pot_bb": gs.pot / bb, "spr": gs.stacks[gs.actor as usize] / gs.pot.max(1.0),
                 "pot_fraction_to_call": to_call / (gs.pot + to_call).max(1.0),
                 "facing": describe_facing(gs) },               // history_signature + history
        "stacks": { "hero_stack_bb": gs.stacks[0] / bb,
                     "villain_stack_bb": gs.stacks[1] / bb, "eff_stack_bb": eff },
        "tournament": json_tc(tc),
        "villain": { "hands_seen": stats.hands_seen, "stats": json_stats(stats),
                     "line_history": describe_line(gs), "recent_showdowns": "..." },
        "hero_image": { /* ImageTracker snapshot (JT4) */ },
        "engine": {
            "blueprint_top_actions": top.map(|(b, p)| json!({
                "bucket": bucket_name.get(b).unwrap_or(&"other"), "p": p })).collect::<Vec<_>>(),
            "candidate_margin": /* pmf[0]-pmf[1] */,
            "legal_size_ladder_bb": legal_ladder_bb(gs, bb),    // mirrors legal_actions fractions
            "off_tree_opponent_action": describe_off_tree(gs),  // pkr-core action_bucket helper
        }
    })
}
```

**3) Texture fns (NEW, pure, unit-tested) — `crates/pkr-meta/src/texture.rs`:**
```rust
/// Card decode must match Deck ordering (pkr-core/src/deck.rs):
/// suits outer [Spade,Heart,Diamond,Club], ranks inner [Two=0..Ace=12].
pub fn rank(c: u8) -> u8 { c % 13 }
pub fn suit(c: u8) -> u8 { c / 13 }

pub struct TextureLabels { pub paired: bool, pub two_tone: bool,
    pub straight_possible: bool, pub flush_draw_possible: bool, pub label: &'static str }

pub fn classify(board: &[u8], board_len: usize) -> TextureLabels { /* count ranks/suits,
    gap analysis; pure fn, no allocation */ }
```
Test: `Deck::new()` order ⇒ card 0 = (Spade, Two), card 13 = (Heart, Two); fixed boards
("Ah Kh 2s 2c 7d"-style literals built from indices) pin each flag.

**4) Two tiny engine wrappers (the only changes outside `pkr-meta`):**
- `pkr-abstraction/src/lib.rs`: `pub fn cluster_id(&self, hole: &[u8], board: &[u8]) -> u64`
  (reuses private `flat_index_*` + `nearest_centroid`; also returns `flop_bucket` id) — fills
  `hand_summary.preflop_bucket_hint` and `engine.bucket_ids`.
- `pkr-core/src/state.rs`: `pub fn action_bucket(fraction_of_pot: f32, is_allin: bool) -> u8`
  — extracts the constant table from `abstract_action_index_static` so `pkr-meta` reads it
  instead of duplicating it (fills `engine.off_tree_opponent_action`).
Both are additive; existing tests must stay green.

**Verify:**
```bash
cargo test -p pkr-meta state      # golden JSON: fixed GameState+TournamentContext -> byte-exact snapshot
cargo test -p pkr-meta texture    # board flags on pinned boards
cargo test -p pkr-abstraction     # cluster_id wrapper: river id == hand_rank>>6 mix, as in get_infoset_hash
cargo test -p pkr-core            # action_bucket helper matches abstract_action_index_static on all 6 buckets
```

---

### JT2 — `MetaAdvisor` trait + gates + fallback (advisor.rs, cache.rs)  🟢

**Depends on:** JT0/JT1.
```rust
pub struct AdvisorOutput {
    pub preset: Option<ExploitPreset>,   // M2
    pub blueprint_override: Option<&'static str>, // M1
    pub size_hint: Option<SizeClass>,    // M3
    pub translate_anchor: Option<Anchor>, // M4
    pub run_resolve: bool,               // M5
    pub log: DecisionLog,                // always: full answers for M12
}

pub trait MetaAdvisor: Send + Sync {
    fn advise(&self, input: &DecisionInput) -> AdvisorOutput;
}
```
- `JevAdvisor` implements it: builds state → checks `cache.rs` (key =
  `(street, texture_class, villain_archetype, stack_bucket, size_class)`; TTL one orbit) → on
  miss calls `client.decide` → applies §6.4 thresholds → returns. `None` response ⇒ all-`None`
  output (pure fallback).
- `NoopAdvisor` returns all-`None` — this is today's behavior, selected by
  `--meta off` or missing `TYPESAFE_API_KEY`.
- Per-feature kill switches: `JEV_FEATURE_ROUTER`, `JEV_FEATURE_EXPLOIT`, `JEV_FEATURE_SIZE`,
  `JEV_FEATURE_TRANSLATE`, `JEV_FEATURE_RESOLVE` (default "0").

**Verify:**
```bash
cargo test -p pkr-meta advisor   # NoopAdvisor == current behavior; low-confidence fixture => fallback
```

---

### JT3 — Wire hooks in trainer play mode + runtime top-k  🟡

**Depends on:** JT2. **Files:** `binaries/pkr-trainer/src/main.rs`,
`crates/pkr-runtime/src/lookup.rs`.
1. `lookup.rs`: add `pub fn top_k_candidates(&self, infoset_key: &Key, k: usize) -> Vec<(ActionId, f64)>`
   (normalize the stored strategy-sum column; already loaded via mmap — read-only change) +
   `candidate_margin() = p1 - p2`.
2. `main.rs` play loop: after blueprint action is computed, if `--meta shadow` run advisor, log
   only; if `--meta live`, apply `AdvisorOutput`. Log line format:
   `JEV|ts|hand_id|question_id|choice|p*|conf|applied|fallback_reason`.
**Verify:**
```bash
cargo build --workspace
cargo run -p pkr-trainer -- play --meta shadow --hands 50   # runs with NoopAdvisor offline; logs JEV rows with applied=0
```

---

### JT4 — Opponent stats + image tracker (stats.rs, presets.rs)  🟢

**Depends on:** JT2. Sliding-window per seat: VPIP/PFR/3bet/WTSD/fold-to-river-bet from the
trainer's hand feed (pure incremental counters; no allocation per hand beyond the window ring).
`presets.rs`: `archetype+leak -> MixShift { fold_boost_river: f32, bluff_boost_turn: f32, ... }`
constants starting at ±5% — small, auditable, A/B-able (§9).
**Verify:** `cargo test -p pkr-meta stats` — synthetic feed of 200 hands produces expected
window stats within ε.

---

### JT5 — Local ICM calculator (icm.rs)  🟢

**Depends on:** nothing (can go first). Exact ICM over ≤ 27 stacks × payout table:
```rust
pub fn icm_equities(stacks_bb: &[f64], payouts: &[f64]) -> Vec<f64>; // 0..=1 each
pub fn bubble_facts(stacks_bb: &[f64], payouts: &[f64], hero: usize) -> BubbleFacts;
// BubbleFacts { next_tier_ratio, places_to_cash, hero_icm_share, icm_pressure_norm }
```
Standard recursive Malmuth–Harville formula; these numbers go into state as facts (doctrine R3 —
Jev judges, it does not compute).
**Verify:** `cargo test -p pkr-meta icm` — 3 stack/4 stack closed forms vs brute-force
enumeration on small cases; known example from the literature.

---

### JT6 — Triage batcher + calibration metrics (triage.rs; `crates/pkr-exploit/src/brier.rs`)  🟢

**Depends on:** JT0. 
- `triage.rs`: read JSONL hand records (§5.3) → per hand one call with `TRIAGE_PACK` (batching =
  chunked at rate-limit safety: ≤ 900 req/min, exponential backoff) → emit `leak_weights.json`
  (histogram × severity mean, normalized).
- `brier.rs`: `brier(prob_of_outcome)`, log-loss, per-question reliability buckets; input = the
  `DecisionLog` JSONL from JT3 + resolved outcomes.
- `binaries/pkr-trainer`: new flag `--sampling-weights leak_weights.json` (playbook T1.x already
  touches the sampling module — same hook point, offline only).
**Verify:**
```bash
cargo test -p pkr-meta triage    # 100 synthetic hands through a mock client (impl MetaAdvisor locally)
cargo test -p pkr-exploit brier  # known Brier values on hand-computed fixtures
```

---

## 8. What NOT to ask Jev (poker anti-pattern map)

Documented model limits (§1.3) mapped to tempting-but-wrong poker questions. These are hard
"don'ts" — the engine already computes all of it better.

| ❌ Never ask | Because (documented limit) | ✅ Do instead |
|---|---|---|
| "What's hero's equity vs villain range?" | No arithmetic; equity = combinatorics | EHS²/OCHS bucket from `pkr-abstraction`; pass as `hero_bucket` fact |
| "How many outs / combos does villain have?" | Cannot count reliably | compute in `pkr-core` |
| "What's the pot odds / MDF / alpha here?" | Pure math | compute in Rust; pass as `pot.pot_fraction_to_call` |
| "What's my ICM EV if I call?" | Recursive math, dates of payouts | `icm.rs` (JT5) computes; Jev only classifies pressure |
| "Has villain's VPIP increased?" (raw) | Counting + date/sequence math over history | compute windows in `stats.rs`, send both windows as facts |
| Open-ended "What should hero do here?" | Jev doesn't generate strategies; Choice needs your option list | blueprint supplies candidates; Jev routes/scores among them |
| "How strong is hand X?" as a bare Score 0–9 | Scores are unanchored without situations | criteria describe hands ("second pair on paired board vs capped range") |
| Score interpolation ("0.4 = 40% of aggressive") | Score is for thresholds/ranking, not magnitudes | compare to threshold constants only |
| Anything from opponent chat / free text | Adversarial steering of state | engine-generated fields only (§5.1 rule 5) |
| Multi-factor composites ("rate this spot 1-10 considering range, texture, ICM") | One judgment per question | the P1 pack already decomposes; combine in Rust (composite scoring pattern) |
| "Which card came on the river 3 hands ago" | Sequence/date memory | timeline facts in state |

---

## 9. Rollout protocol — shadow → parity → gated live

**Compliance first (restated):** run bots only where permitted — sims, private leagues, study
tools, sanctioned bot tournaments, your own research environments.

### Phase 0 — Shadow (week 1)
`--meta shadow`: advisor runs on every play-mode decision, **nothing applied**, everything logged.
Goals: (a) latency p50/p95 measured from your location; (b) confidence distributions per question;
(c) first calibration data vs realized outcomes (M12). Exit criteria: p95 added latency ≤ 600 ms;
no unanswered errors > 1% after retries.

### Phase 1 — Parity A/B (week 2–3)
Offline match in `pkr-testgames` (reuse the kuhn harness pattern for NLHE table config):
- Arm A: blueprint-only bot. Arm B: same bot + `--meta live` with all thresholds at their §6.4
  defaults. **≥ 100k hands** (duped opponents/seats, mirrored decks where the harness allows).
- Metric: mb/hand delta with 95% CI. Acceptance: CI lower bound > −1 mb/hand **and** point
  estimate ≥ 0 to enable a feature; per-feature acceptance (enable one feature at a time:
  M1 → M2 → M4 → M5 → M3).
- Calibration gate per question: Brier ≤ 0.20 on ≥ 5k logged outcomes (M12), else that question's
  feature stays off.

### Phase 2 — Gated live (week 4+)
Confidence thresholds live per §6.4; per-feature env switches; kill switch
`JEV_ENABLED=0` (or missing API key) reverts to blueprint-only within one decision.

### Phase 3 — Recalibrate & grow
Nightly: Brier curves; monthly: replay set (below); when TypeSafe ships a new model version:
pin bump requires replay green before deploy (M13). Feed M10 leak weights into the next training
run (playbook T1.x curriculum hook); consider M11 counter-blueprints once T2/T3 pipelines are solid.

### Replay set (behavior-preservation test)
Freeze 500 anonymized decision states + their logged answer distributions. A CI job replays them
(against the mock, and weekly against the pinned live model) and asserts: all answers parse;
option sets unchanged; per-question mean probability within ±0.15 of the frozen baseline. Any
drift ⇒ threshold re-tune or version rollback. This is the practical defense behind TypeSafe's
"version questions, criteria, thresholds together" guidance.

---

## 10. Cost, latency, and the M1 16GB budget

**Runtime decision (P1+state ≈ 1.6–2.4k input tokens):**

| Item | Value |
|---|---|
| Price | $0.042 / 1M input tokens, output free → **≈ $0.00007–0.00010 / decision** |
| 10k-hand session | ≈ $0.70–1.00 |
| Cache hit rate (per-orbit TTL) | typically 30–60% on early streets → real cost lower |
| Added latency | ~100–300 ms typical (70–500 ms doc range + RTT) vs 15–30 s online timebanks |
| Rate limits | 1,200 req/min ≫ tournament decision rate (~2–4 req/min) |

**Offline triage (M10, P3 ≈ 1.2k tokens/hand):** 1M hands ≈ $50 and ≈ 14 h of wall-clock at
900 req/min — run overnight alongside M1 training without contending for CPU (network-bound).

**M1 impact: zero.** Jev calls are remote: no RAM, no GPU, no thermal budget consumed; the
training loop (§3 "never touched") is unaffected; the runtime cache is a few hundred KiB.
This *complements* the playbook's memory plan rather than competing with it — the meta layer is
the rare upgrade that costs 0 of your 16 GB.

**Failure economics:** worst case (API down mid-session) = today's bot, immediately (R2
fallbacks). Best case = population- and stage-aware play your static blueprint cannot produce.

---

## 11. Sources (accessed 2026-09-23)

1. TypeSafe AI homepage & launch page — typesafe.ai (System One models, RLCD, benchmarks, FAQ).
2. TypeSafe docs — docs.typesafe.ai: `/introduction`, `/concepts/state`, `/confidence`,
   `/patterns` (speculative fan-out, confidence-gated routing, composite scoring), SDK pages;
   Mintlify `.md` mirrors; `/llms.txt`.
3. TypeSafe blog — "Introducing System One Models & Jev" (Sep 15, 2026).
4. F. Copes, "A deep dive into Jev, TypeSafe's System One model" — flaviocopes.com/jev (+/jev.md):
   request/response shapes, state limits (64k/32k tokens, 255 options, 2–10 score levels),
   jaggedness list, pricing/rate limits, skeptical notes, Vercel AI SDK usage, ZDR note.
5. madewithjev.com catalog — 473 builds; "Games and real time" category (Tetris, driving sim,
   Doom bot); OpenJEV playground (openjev.sh).
6. jevusers.com — community catalog incl. "Jev test bench + Claude/LLM-and-Jev collaboration
   measurement harness"; "Jev Test Bench — Texas Hold'em Edition" (GitHub, via Vercel AI Gateway).
7. Third-party explainers: DataCamp ("System One model that never hallucinates"), LangChain blog
   guide, MindStudio (non-autoregressive), Requesty (probability semantics), CellCog
   (state + typed questions model, $0.042/M), Pydantic docs TypeSafe page (`typesafe-sdk`),
   Hugging Face "first structured decision" tutorial, Apidog API-key walkthrough,
   explainx (Vercel + LangChain routing integration).
8. TypeSafe agent skill — `npx skills add typesafe-ai/skills --skill typesafe-ai`;
   console.typesafe.ai; evals.typesafe.ai.
9. Background for the exploitation/abstraction rationale: RL-CFR (arXiv 2024), GTO Wizard solver
   blog (nodelocking/real-time solving), poker-ai.org CFR articles — cited in the companion
   playbook §6.
