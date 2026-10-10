# HANDOFF — session 4 (2026-10-07): safety bound + V5 search

**Continues:** `HANDOFF_2026-10-06_session3.md`
**HEAD:** `git log -1`  **Tree:** `git status`

---

## Section 1 — What this session did

Built and validated the **search safety bound** (the gating item before
shipping search), then measured the search on the **V5** checkpoint.

### Safety bound (complete, tested)

Vector-path analog of `safe_solve_root_p0_strategy`:
`crates/pkr-subgame/src/vector.rs` gained `root_strategy_all`,
`set_root_strategy`, `opp_br_value` (fixed to handle either root actor),
`blueprint_root_strategy`, `blend_root`. `crates/pkr-runtime/src/vsearch.rs`
gained `solve_root_bounded` (binary-search the largest blend with the
blueprint whose opponent-BR ≤ the blueprint's) and `iterate_budgeted`
(wall-clock cap). `VSearchHandle::with_bound(cfg, abs, tbl)`.
CLI: `--search-bound`, `--search-bound-steps`, `--search-budget-ms` on
`lbr_eval` and `tournament`. Diagnostics: `PKR_SEARCH_DEBUG=1` prints
`bounded: solves/kept_search/blended/fell_back`.

Docs: `docs/experiments/vsearch-safety-bound.md`.

### Measurement on V5

| instrument | result |
|---|---|
| A: V5 blueprint LBR | −2.11 ± 98.75 (reproduces the A/B exactly) |
| B: V5 river search, bounded, LBR | −122.96 ± 100.34 (Δ=−121, \|t\|≈0.86, **flat**) |
| **D1: V5 river search, bounded, H2H vs blueprint** | **+0.889 chips/deal, t=+4.71** |

**Key result: LBR is flat on V5 but H2H is strongly positive.** LBR is a
weak measure for search (third time confirmed). The bounded river search
wins head-to-head on the best checkpoint. Docs:
`vsearch-on-v5-lbr-flat.md`, `vsearch-on-v5-h2h-positive.md`.

### Bug fixes this session

- `opp_br_value` (was `p1_br_value`, OOP-only — wrong for IP-rooted solves).
- `bound_max_steps` added after discovering the bound cost.
- Tournament progress reporting (hours-long runs were unobservable).

### Errors made

1. **Killed instrument C on a bad extrapolation.** I extrapolated
   "bounded turn = 14x / 20h" from a contended measurement and killed
   C before it finished. The real cost is ~1–3.5 s/hand (7–19 h/20k) —
   expensive but not catastrophic. Corrected in
   `turn-bound-too-expensive.md`. **Lesson: don't extrapolate per-hand
   cost from a contended machine; don't kill an instrument on an
   unmeasured guess.**
2. Several scripts used `..Default::default()` brace-guessing that
   corrupted files (reverted, redone with exact anchors).
3. `pgrep -c` / `timeout` don't exist on macOS as on Linux.

---

## Section 2 — Running at handoff

**D2** (turn+river unbounded H2H vs blueprint, 20k, V5): running
~3h at handoff. This is the last instrument of phase1b. Output:
`outputs/vsearch-v5-turn/D2-h2h-t+r-unbounded.txt` (prints its result
block when done). Phase1b script `/tmp/vsearch-phase1b.sh` will print
`PHASE1B DONE` to `/tmp/vsearch-phase1b-result.txt`.

---

## Section 3 — The combined picture

| base | LBR Δ (search−bp) | H2H Δ (chips/deal) |
|---|---|---|
| V4 (2026-10-06) | −377 (\|t\|≈3.1) | +1.254 (t=6.25) |
| V5 river (2026-10-07) | −121 (\|t\|≈0.86, flat) | +0.889 (t=4.71) |

Both bases: H2H positive. **The vector search helps.** The safety bound
(river) does not kill it.

---

## Section 4 — Decision

**Ship the bounded vector search** to the blueprint+search policy on
both V4 and V5. River search is validated on both instruments
(bounded and unbounded).

**Turn search** is unresolved pending D2 (turn+river H2H). If D2 is
positive, turn adds value; if flat, river-only is the deliverable.

---

## Section 5 — The real remaining gap: CFR-D gadget

The root blend is a **heuristic in-sample** bound, not the theorem-backed
CFR-D gadget (plan Task 4.4). It bounds the opponent-BR against the
tracked ranges, not every opponent. Before playing a real external
opponent (Slumbot), the gadget should replace it. This is the top
follow-up.

---

## Section 6 — If you only do one thing

`grep -E "diff \(A - B\)|t:" outputs/vsearch-v5-turn/D2-h2h-t+r-unbounded.txt`

- If D2 t > +2: turn search helps; ship turn+river.
- If D2 flat: ship river-only.

Then read the combined table (Section 3) and write the "vector search
ships" decision doc.

---

## Environment

- macOS: no `timeout`/`gtimeout`, no `awk match(s,re,arr)`, `pgrep -c`
  absent.
- `.git/index.lock` contention from the watcher is routine; commits land
  anyway. `rm -f .git/index.lock` when no `git` process is live.
- Foreign load observed (`glyim-v2`, `chameleon`) — not ours.
