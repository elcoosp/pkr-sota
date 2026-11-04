# pkr-sota: History & Verified Findings

**Last updated:** 2026-11-03 (docs synthesis). This file preserves the
durable content of the old `docs/experiments/`, `docs/handoffs/`,
`docs/roadmap/`, `docs/reviews/`, `docs/spec/` and `docs/playbooks/`
trees, with stale numbers corrected. Every number here carries its
measurement context — read the caveats in [`STATUS.md`](STATUS.md) first.

---

## 1. Timeline (session narrative)

| date | session | outcome |
|---|---|---|
| 2026-09-24 | post-audit landing | F1–F9 audit fixes (A2–B10, C2–C3, D1–D2, E1–E4); fast7 evaluator bugs found & fixed (encoding mismatch, missed straight-flush, COMBOS_7_5 OOB); perf landing P1-f etc. |
| 2026-09-24→10-02 | v33–v42 run series | abstraction rebuilds, capacity sweeps, rich 6D preflop landed (v33 confirmed); T2.2 river-resolution tried and REVERTED (neutral) |
| 2026-10-01→02 | sessions 1–3 | bug-hunts; estimator corrected (opponent reach); tournament/LBR/CRN H2H harnesses built (F9); post-audit invalidation of pre-estimator-fix numbers |
| 2026-10-03→06 | sessions 3–4 | search arc: vsearch executes, measurement wired, safety bound proven, shipped-safe config frozen; a H2H invalidated by a `pkr-bot-pipe` seat bug (fixed); Phase 4.3 tracker design |
| 2026-10-07→08 | sessions 4–5 | CFR-D gadget built; terminate-value bug found (BR vs EV); K=6 numbers verified (VERIFIED_FINDINGS 10-09) |
| 2026-10-09→10 | sessions 6–8 | vector gadget PROVEN safe; **K=7 shipped** (both plan §3 gates cleared); K=7 landed review fixes; session 8 |
| 2026-10-10→12 | session 9 | ~50 review fixes landed (all Critical C1–C10, non-deferred Majors, ~25 Minors); M-31 arena refactor (aliasing UB removed); M-15 streaming checkpoint loader; M-3 strict min-raise opt-in; Phase 4.3 tracker fix; perf P5–P18 triage closed; **run 1 negative (DO NOT SHIP)** |
| 2026-11-03 | docs synthesis + re-audit | 130+ stale docs synthesized into ARCHITECTURE/STATUS/HISTORY; full codebase re-audit; residual bugs fixed TDD (see CHANGELOG) |

---

## 2. Verified findings (K=6 era — authoritative for K=6 only)

From the 2026-10-09 verification pass. K=6 and K=7 are different
abstract games (stride + infoset key both differ): they cannot meet in
`tournament` or `lbr_eval` and their numbers must never be compared.

### Search
- Raw vector river search, on-tree BR vs V5 blueprint: 2.16 vs 8.44
  (search is 4× less exploitable). 4-subgame sweep: raw < blueprint on
  all.
- Search H2H vs blueprint, V5, 20k deals, BOUND config, 4 seeds:
  +1.1763 / +0.8211 / +0.9722 / +1.1625 chips/deal
  (t = 6.16 / 4.11 / 4.96 / 6.17).
- Search H2H, GADGET config (corrected EV terminate value), seed 42:
  +0.6203 (t = 5.14).
- V4: LBR 821 → 444; H2H +1.254 (t = 6.25).
- Arena vs scripted: blueprint 69.25 vs +search 102.27 bb/100
  (300 hands).

### Safety (both PROVEN)
- BOUND: `final_br ≤ blueprint_br` (test: 0.0205 vs 0.0245).
- GADGET (sampled): bp_br 9.5612 vs gadget_br 0.6284.
- GADGET (vector): gadget ≤ blueprint in all 4 sweep subgames (worst
  −0.94).
- The gadget bug was the terminate value: BR is WRONG, EV is correct
  (cbv=BR → 11.23, cbv=EV → 2.09, raw → 2.16).

### Live runtime (measured)
- River decision (bound): 0.69 s after the budget fix (was 2.14 s).
- Turn decision (bound): 44.6 s even with the budget → OFFLINE ONLY.
- Live default: river-only, proven bound. `PKR_VSEARCH_RAW=1` /
  `PKR_VSEARCH_GADGET=1` to opt out; `PKR_VSEARCH_STREETS` to widen.

### V4 vs V5 (the signature question)
- Cross-process H2H (examples/v4v5_h2h.rs, CRN, fixed bot, 10k × 4
  seeds): +0.27 / +0.08 / +0.16 / +0.10, all |t| < 1 → EQUAL in
  self-play.
- LBR samples=2000: V4 = 997, V5 = 183 (gap 814). In-tree only:
  V4 = 227, V5 = 29 (gap 198). Off-tree = 76% of the gap.
- Read: V5 is genuinely less exploitable (in-tree and off-tree) but
  does not beat V4 head-to-head. Different axes; not a contradiction.

### K=7 baseline (shipped 2026-10-10)
- LBR (20k deals, seed 42): −162.79 ± 104.49 mbb.
- H2H search-vs-blueprint: +0.958 (t = +5.28) seed 42; +1.279
  (t = +7.02) seed 43. Both plan §3 gates cleared (LBR ≥ 2·SE,
  H2H > 2·SE).
- K=7 constants: `NUM_ACTION_BUCKETS` 7, `BET_SIZINGS` [0.5, 1, 2, 3],
  bucket 6 = jam. Checkpoint-invalidating by design.

### Run 1 (2026-10-12): DO NOT SHIP
Combined `eps=0.002 + PKR_STRICT_MIN_RAISE=1` at 100M, seed 42:
LBR +98.54 ± 114.27 (vs −162.79 baseline; +261 mbb regression,
reproduced byte-for-byte with the current binary); H2H +1.105/+1.078
(unchanged); in-sample BR 8062 vs 7922 (worse at every eval point).
Two independent measures agree it is a mild regression. The knobs were
bundled, so eps vs strict is not isolated.

### Training-config sweeps (no shipping signal)
- **Eps sweep** (2026-10-11, 20M, 6 arms): lower eps trends better but
  the result is dominated by seed variance — NOT a shipping signal.
- **Leduc CFR grid** (2026-10-10): eps=0.002 is ~37% better than 0.01
  *on Leduc*; did not transfer as an NLHE signal (run 1).
- **Capacity sweep** (v36): informed `--iters-per-sync` 2048 default.

---

## 3. Performance triage (P5–P18, closed 2026-10-11)

| item | verdict |
|---|---|
| P1 FMph verify-then-use | landed |
| P7 (batch tweak) | tried, measurably slower → reverted |
| P10 signature dispatch cached | landed |
| P13 partial — 1326-combo table cached | landed |
| P17 `madvise(MADV_RANDOM)` | landed |
| P5 / P8 / P11 / P15 / P16 | verified not actionable |
| P9 UndoRecord slimming | deferred (checkpoint format versioning needed) |

Earlier perf landings (2026-09-24): P1-f action-indices table removed
(~4 GB/s of memset saved), UndoRecord field shrink with a size
assertion test, `add_sum_grouped` non-CAS batch path, targeted
`#[inline]`s. Effect: ~48–61K it/s vs ~28–30K baseline (M1).

---

## 4. Negative results register (deliberate dead ends)

These are recorded so nobody re-runs them blindly:

- **T2.2 finer river resolution** (`>>15` → `>>13`, buckets 200 → 128):
  neutral at eval, reverted; `RIVER_TIER_SHIFT` is 15 in code and is
  encoded in the fingerprint.
- **PCFR+ momentum** (`PKR_MOMENTUM`): no NLHE win; off by default.
- **Variance reduction (AIVAT)**: negative; plan archived.
- **k=400 abstraction**: not actionable at current capacity.
- **Turn-time search**: 44.6 s/decision → offline-only, even bounded.
- **Held-out BR split (C1)**: reverted — infoset hash is keyed on
  `cluster_id`, so a BR policy fit on deal set A does not generalize to
  deal set B.
- **Suit isomorphism port (Task 2.3a)**: reverted — tests caught
  over-identification of non-equivalent hands.
- **Eps=0.002 + strict min-raise combined (run 1)**: mild regression,
  DO NOT SHIP.
- **P7 perf idea**: measurably slower, reverted.

---

## 5. Blueprint compatibility

The infoset-hash scheme changed over time; blueprints are loadable only
by code whose hash matches the producer. The runtime rejects newer
formats loudly, validates the abstraction fingerprint, and a stale-but-
loadable file is diagnosed by `blueprint_hits: 0` in the eval harness
(0% hit rate = you are measuring the fallback policy, not the trained
strategy).

| blueprint | produced with | loadable with current code |
|---|---|---|
| v0–v8 | pre-K=7 hash schemes (incl. old river shifts, flop_bucket) | No |
| K=6 era (v9–v15 format) | K=6 abstraction (6 buckets) | No — different abstract game |
| current (v4 format) | K=7 abstraction, `RIVER_TIER_SHIFT=15` | **Yes** — writer emits v4 only |

Practical rules:

- Any blueprint whose fingerprint does not match the code is rejected at
  load (`InvalidData`), never silently mis-hashed.
- Future hash changes must bump `FORMAT_VERSION` in
  `crates/pkr-export/src/header.rs`.
- To check provenance: query a hash the current abstraction produces;
  `lookup(hash) == None` on a loadable file means the file is stale.

---

## 6. Methodology lessons (measurement)

1. **LBR alone is not trustworthy** — one greedy exploiter; misled the
   project 3× in one week. Always pair with the cross-process H2H
   (common random numbers, fixed bot).
2. **In-sample BR is not a strength measure** — it fell 3796 → 1707 →
   1222 as eval deals went 5k → 20k → 40k. Relative, seeded A/B only.
3. **Tournament seeds 42–45 were correlated** (shifted-by-one); fixed.
4. **`pkr-bot-pipe` seat bug** gave seat 1 the wrong hole cards; fixed;
   one H2H run invalidated.
5. **Bundled A/B knobs cannot be attributed** (run 1 lesson): change one
   thing per run, or budget for a factorial design.
6. **Test temp-file races**: tests touching the filesystem must use
   `tempfile` or per-pid names (parallel nextest binaries collided).
7. **Zero-hit-rate evals are silent fallbacks**: `blueprint_hits: 0`
   means you measured the fallback policy.

---

## 7. Reviews (what the external reviews said)

- **Code review (2026-10-10, @ cce1339)**: 3 High / 6 Medium / 7 Low /
  15 smells / 6 doc bugs. Disposition: every Critical (C1–C10) and every
  non-deferred Major fixed by session 9 (~50 fixes); residuals
  re-audited and closed in the 2026-11-03 pass (see CHANGELOG).
- **Competitiveness review (2026-10-02)**: thesis — the honest ceiling
  of the current design is "provably not trivially exploitable", not
  superhuman play; called out in-sample estimator bias (later confirmed
  and fixed), memory under-use, and the infoset-merge critique. The
  project's honesty rules in STATUS.md codify this.
- **Competitiveness review 2 (2026-10-03)**: second pass on the same
  questions with a fresh build; conclusions consistent.
- **Accelerated best response (playbook, 2026-10-02)**: literature
  summary for public-tree BR — retained idea: public-state BR is the
  cheap exploitability instrument if exact enumeration is too slow.

---

## 8. Process notes (from the old playbooks, condensed)

- **Inner loop**: `just fast` (fmt + clippy `-D warnings` + nextest).
  CI `fast` workflow is the same gate.
- **A/B protocol** for any perf or quality change: fixed seed(s), fixed
  eval config, 2·SE promotion gate, record both arms in an experiment
  doc (now: append to CHANGELOG + STATUS).
- **Anti-hardcode gate**: `ci/scripts/check-no-hardcoded-k.sh` refuses
  hard-coded action-bucket counts — new code must use the shared
  constant (the K=6→K=7 lesson).
- **Golden hashes**: any intentional numerics change requires
  regenerating `ci/golden-training.sha256` with `GOLDEN_UPDATE=1` and
  saying so in the CHANGELOG.
- **Checkpoint hygiene**: every K=7 checkpoint is ~1.9 GB with a `.prev`
  rotation copy — clean `.prev` and `blueprint.best.bin` after runs.
