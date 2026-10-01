# HANDOFF — audit follow-up session (2026-10-01)

**Continues:** `docs/HANDOFF_2026-09-30_audit.md`
**Supersedes:** none

---

## What this session did

Continued the competitiveness-audit response from 2026-09-30. Where the
prior session landed fixes for F1-F9, this session:

1. **Closed F5.** Kuhn grid + NLHE A/B both say keep the current
   update rules.
2. **Built F4's rebuild pipeline.** New precompute subcommands,
   regenerated flop table, verified 99.6% of buckets changed.
3. **Launched v45** — the F4 A/B against v42.
4. **Made the fingerprint safe for F4.** Env-driven
   `centroid_feature_v` so a future load of the new checkpoint against
   the old tables fails the guard.
5. **Added regression tests** for F6, F7, F8, F2 defaults, and
   fingerprint guards.
6. **First real-game evaluation** — `pkr-arena` against the v42
   checkpoint, +210 bb/100 with 99.9% blueprint hit rate.

---

## F5 — CLOSED

Two independent results:

**Kuhn grid** (`docs/experiments/f5-grid.md`):

| iters | neg_floor=true (RM+) | neg_floor=false |
|---|---|---|
| 1e5 | 0.000398 | 0.000338 |
| 1e6 | **0.000199** | 0.000323 |

RM+ floor helps on Kuhn. Keep the default.

**NLHE A/B** (`docs/experiments/v42-vs-v43-ab.md`):

| iter | v42 (traverser) | v43 (opponent) |
|---|---|---|
| 3.0M | **3313.4** | 3453.8 |
| 6.0M | 3430.3 | **3402.7** |
| 18.0M | 3780.4 | 3723.7 |
| **best** | **3313.4** | **3402.7** |

Delta 89 mbb, within 1 SE (130). Statistically equivalent.

**Both curves turn up at 3-6M.** The rise is not the averaging site.
Combined with the Kuhn result, F5 is closed for both dimensions. The
flags stay at their defaults.

---

## F4 — REBUILD DONE, v45 TRAINING

**New subcommands** (`crates/pkr-abstraction/src/bin/precompute.rs`):

- `centroids-potential <num_flops> <k> <rank_table> <output>` —
  fits k-means on (mean, potential) features. 500 flops × 200 hands =
  100k pairs, ~6s at k=200.
- `abs-potential <centroids> <rank_table> <output>` — builds the flop
  hand table against those centroids. 26M entries, ~9 minutes on 8
  cores.

**Rebuild result** (`outputs/v44-potential/`):

| | old (EHS, EHS²) | new (mean, potential) |
|---|---|---|
| bytes differing | — | 25,892,395 / 25,989,600 (99.6%) |
| distinct buckets | 193 | 200 |
| bucket entropy | 6.687 bits | 6.557 bits |

99.6% of bucket assignments changed. The potential feature is real.

**v45 training** launched against the new table. Same config as v42
except the flop table. `outputs/v45-potential/`.

Watcher at `/tmp/v45-result.txt`.

---

## F1 — PINNED

`crates/pkr-exploit/tests/f1_estimator_pin.rs` (commit `e7b42e5`)
loads the v42 checkpoint, runs `sampled_exploitability(500 deals,
seed 42)`, and asserts 10253.8 ± 30 mbb. `#[ignore]`d — takes ~2.5
min because checkpoint load dominates.

Any change to the reach weighting, per-deal accumulation, or tree walk
shifts the reading. This is the durable F1 guard.

---

## Regression tests added

| commit | what |
|---|---|
| `b5a0f29` | F6 legal-action-tree (3 tests: jam always legal, min-raise clamps) |
| `e3d4601` | F7 allocation helpers (4 tests, one ignored for large-capacity) |
| `80d36dc` | F8 purify (4 tests: threshold behaviour, 2-action protection) |
| `57e4374` | F2 experiment defaults pinned field-by-field |
| `73669a0` | Fingerprint guards (action_legal_v, centroid_feature_v) |
| `fb892be` | F5 Kuhn grid (env-driven, run twice) |
| `dd3f9d5` | Tournament A-vs-A must be zero |
| `b748d5a` | Tournament zero-variance case is not "run more hands" |

---

## Evaluation harness (F9) — FIRST REAL-GAME RESULT

`crates/pkr-fuzz/src/bin/arena.rs` — `pkr-arena --checkpoint <path>
[--hands N] [--seed S]`. Prints per-opponent bb/100 and blueprint hit
rate.

`crates/pkr-fuzz/src/bin/tournament.rs` — `pkr-tournament --a <ckpt>
--b <ckpt>` — head-to-head duplicate match.

**First real-game eval ever** (`docs/experiments/first-real-game-eval.md`):

    vs StationBot:  +277.55 bb/100
    vs NitBot:       +36.76 bb/100
    vs AggroBot:    +266.50 bb/100
    aggregate:      +193.60 bb/100
    hit rate:       99.9% (11162/11176 decisions)

Re-run against v42's final checkpoint: +210.03 bb/100.

The hit rate is load-bearing — a low rate would mean the arena was
measuring the fallback path, not the trained strategy.

---

## Fingerprint: env-driven `centroid_feature_v`

Commit `dcef4af`. `AbstractionFingerprint::from_constants` reads
`PKR_CENTROID_FEATURE_V` (0 = legacy, 1 = potential). Every component
(trainer, arena, tournament) agrees because they all call the same
constructor. The v45 launcher exports `PKR_CENTROID_FEATURE_V=1`;
existing runs are unchanged because the default is 0.

Without this, an F4 checkpoint loaded against legacy tables would not
have failed the guard.

---

## What's running at handoff

`outputs/v45-potential/` — the F4 A/B against v42. Same config,
different flop table. 30M iterations, seed 42, plateau-stop 5.
Watcher at `/tmp/v45-result.txt`.

---

## What the v45 result will tell us

- **If v45 best < v42 best - 2·SE**: the F4 feature rebuild helps.
  Ship `PKR_CENTROID_FEATURE_V=1` as the default for new runs.
- **If v45 best within ±2·SE of v42**: the new feature is neutral.
  The old table is fine; keep it (more checkpoints match).
- **If v45 best > v42 best + 2·SE**: the new feature hurts. Revert
  F4's rebuild direction; the previous feature set was better.

v42's best is 3313.4 mbb; SE is ~130.

---

## Not done (next session)

1. **The turn-table rebuild.** The F4 plan measured the turn rebuild
   at ~2.5 days of compute. Only the flop was rebuilt. If v45 helps,
   the turn is the next step. Would need its own `abs-potential-turn`
   subcommand or an extension to `turn`.
2. **Suit isomorphism.** Would cut the turn rebuild by ~20x. Not
   implemented; there's no board-canonicalization infra in the repo.
3. **F3 flip and retrain.** `SIG_V3_SIZE_AWARE = true` invalidates
   every checkpoint. Needs v45 to settle first so we know which F4
   tables to train F3 on.
4. **Runtime tracker integration.** `RuntimeSession` exists; no bot
   binary uses it.
5. **`pkr-arena` vs the new checkpoint on the same day v45 lands**
   — the +210 vs +193.6 comparison shows the arena itself varies by
   ~8% across checkpoints. Worth running at higher hand count.

---

## Session meta

- Commits: ~12
- Findings closed: 1 (F5)
- Findings partially closed: 1 (F4 — pipeline + rebuild + A/B launched)
- Real bugs found: 0 (this session added tests and features)
- Regression tests added: 20+
- First real-game result in project history: yes

## Reference

| path | what |
|---|---|
| `docs/experiments/f5-grid.md` | F5 conclusion |
| `docs/experiments/v42-vs-v43-ab.md` | F5 NLHE A/B |
| `docs/experiments/v42-post-audit-result.md` | v42 curve |
| `docs/experiments/f4-abstraction-rebuild-plan.md` | F4 rebuild |
| `docs/experiments/first-real-game-eval.md` | arena result |
| `docs/HANDOFF_2026-09-30_audit.md` | prior session, F1-F9 fixes |
| `crates/pkr-fuzz/src/bin/arena.rs` | evaluator |
| `crates/pkr-fuzz/src/bin/tournament.rs` | head-to-head |
| `crates/pkr-fuzz/src/provider.rs` | shared `TableProvider` |
