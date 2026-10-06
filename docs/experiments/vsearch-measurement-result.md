# Vector search — MEASUREMENT RESULT (2026-10-06)

**Instrument:** `lbr_eval --search-vector --search-streets r --search-iters 100`
and `tournament --search-a --search-vector ...`, against the fixed
checkpoint `outputs/ab-v4/train.ckpt` (V4 sig, 100M iters), 20k hands /
deals, seed 42, 400 samples.

## Result

**Paired LBR** (20,000 hands, seed 42):

| arm | mbb/hand | SE |
|---|---|---|
| blueprint-only | 821.08 | 84.24 |
| **vector river search** | **444.33** | 88.63 |

Δ = **−376.75 mbb**, combined SE ≈ 122, **|t| ≈ 3.1**. The search is
measurably less exploitable. Decision rule needs Δ ≤ −2·SE → **passes**.

**Head-to-head** (20,000 duplicate deals, seats swapped, A = search,
B = blueprint, same checkpoint):

| | value |
|---|---|
| mean diff (A − B) | **+1.2540 chips/deal** |
| SE | 0.2006 |
| **t** | **+6.25** |

Search wins head-to-head. Decision rule needs diff > +2·SE → **passes**.

**Plumbing sanity** (search flag ON, all streets disabled, 2,000 hands):
262.81 ± 322.26 — consistent with the 20k blueprint reading within
noise (2k is ~7× wider SE). The hook does not perturb the walker when
the gate refuses. Passes.

## Verdict: SHIP THE VECTOR SEARCH (to the blueprint+search policy)

This is the **first search variant to pass both instruments**. The
sampled-hand solver passed LBR alone (−627 mbb) and then LOST head-to-
head by −6.06 chips/deal (`search-built-negative-h2h.md`). The handoff
§4 decision rule exists precisely to catch that, and the vector search
clears it cleanly: **LBR down AND H2H positive**.

## What this does NOT license

1. **Shipping the raw solve to a real opponent.** `VSearchHandle`
   returns the raw vector solve — no CFR-D gadget, no safety bound.
   The sampled solver had the same deficiency (it used the heuristic
   blend). The positive H2H is *necessary but not sufficient*: the
   next step before any external game is the CFR-D reach-subgame
   gadget (plan Task 4.4).

2. **Turn search.** This measurement is river-only (plan §4.5 allows
   it — river is 1–2 s/solve, turn is 2–4 s/solve). Turn is a follow-up
   with a fresh budget study.

3. **Claiming the effect size on a different checkpoint.** This ran on
   `ab-v4`. The V5 checkpoint (`ab-v5`) won the V4/V5 A/B and would be
   the natural host — but that is a separate measurement (search-on-V5
   vs search-off-V5).

## Config that produced the win

- `bet_fracs = [0.5, 1.0]` (default `VSearchCfg`)
- `max_raises = 2`
- `iters = 100` (converges by 30 per `vsearch-smoke-2026-10-06.md`;
  100 is safely past convergence)
- `enabled_streets = [false, false, false, true]` (river only)
- Ranges: `RangeTracker` posterior for the opponent, point mass for us

## Recommended next steps (priority order)

1. **CFR-D gadget** (plan 4.4). The one correctness prerequisite for
   shipping search to an opponent.
2. **Turn search** (`--search-streets tr`), with a latency budget study
   on the M1.
3. **Search on the V5 checkpoint** (re-run this whole measurement with
   `ab-v5` as base).
4. If turn+river both help, revisit the flop (plan Phase 5,
   depth-limited).
