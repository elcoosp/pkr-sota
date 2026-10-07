# Turn-tree safety bound is too expensive (2026-10-07)

## Finding

`solve_root_bounded` with `--search-streets tr` (turn + river) costs
~14x the unbounded turn solve: the bound calls `opp_br_value()` up to
14 times per solve (2 to measure search vs blueprint roots + up to 12
binary-search steps), each walking the **entire** turn tree (46 river
subtrees).

Measured: instrument C (V5, turn+river, bounded, 20k hands) still
running at **57 min wall / 52 min CPU**; unbounded river search took
8 min. Projected full run ≈ 20 h+ — not viable.

The wall-clock cap (`--search-budget-ms`) does **not** help: it caps
`iterate`, not `opp_br_value`. The bound cost is outside the capped loop.

## Where it bites

Cheap at the **river** (one street; instrument B ran in ~15 min).
Expensive at the **turn** (46 subtrees) and worse at the flop.

## Options for the follow-up

1. **Cap binary-search steps.** Expose `bound_max_steps` (12 default;
   2–3 for turn). Coarser alpha = more conservative, still SAFE
   (`BR <= bp` holds). Cheapest fix.
2. **Bound only at the river.** Turn on raw solve + budget cap. Loses
   safety where the tree is largest — not recommended alone.
3. **Amortise the BR.** Cache per-node BR values for the fixed tree.
   Real work.
4. **CFR-D gadget** (plan 4.4). Theorem-backed; also expensive.

## What the measurement delivered anyway

- **A**: V5 blueprint LBR = −2.11 (reproduced the A/B exactly).
- **B**: V5 river search, bounded = −122.96. Δ = −121, |t| ≈ 0.86 —
  **flat**. V5's blueprint is near-unexploitable by LBR; search has
  little to remove (bound fired 0.9%).

Decisive instrument is H2H (phase1b, running).

---

## CORRECTION (2026-10-07, later)

The "~14x / 20h" figure above was **wrong** — it extrapolated from a
run that was itself competing with other jobs for CPU, and from an
instrument (C) I killed before it finished.

Direct 200-hand timings (bounded turn+river, steps=2 vs steps=12):

| bound_max_steps | wall (200 hands) | rate |
|---|---|---|
| 2 | 697 s | 3.5 s/hand |
| 12 | 265 s | 1.3 s/hand |

The steps=12 run came out FASTER, which is impossible without CPU
contention — another job was running. So even these are unreliable.

What is reliable:
- Bounded turn+river is on the order of **1–3.5 s/hand**, i.e. **7–19 h
  for 20k hands** — expensive but not the 20h+ catastrophe claimed, and
  within an overnight budget.
- The bound at steps=2 and steps=12 gave the IDENTICAL LBR
  (−1385.72 on 200 hands), so the binary search is not changing the
  root much on this checkpoint — the extra steps buy little.
- `bound_max_steps` is still worth having (it bounds worst-case cost),
  but the "14x" framing was wrong.

Lesson: do not extrapolate per-hand cost from a contended machine, and
do not kill an instrument on an unmeasured extrapolation. Instrument C
should have been left to finish.

---

## CORRECTION (2026-10-07, later)

The "~14x / 20h" figure above was **wrong** — it extrapolated from a
run that was itself competing with other jobs for CPU, and from an
instrument (C) I killed before it finished.

Direct 200-hand timings (bounded turn+river, steps=2 vs steps=12):

| bound_max_steps | wall (200 hands) | rate |
|---|---|---|
| 2 | 697 s | 3.5 s/hand |
| 12 | 265 s | 1.3 s/hand |

The steps=12 run came out FASTER, which is impossible without CPU
contention — another job was running. So even these are unreliable.

What is reliable:
- Bounded turn+river is on the order of **1–3.5 s/hand**, i.e. **7–19 h
  for 20k hands** — expensive but not the 20h+ catastrophe claimed, and
  within an overnight budget.
- The bound at steps=2 and steps=12 gave the IDENTICAL LBR
  (−1385.72 on 200 hands), so the binary search is not changing the
  root much on this checkpoint — the extra steps buy little.
- `bound_max_steps` is still worth having (it bounds worst-case cost),
  but the "14x" framing was wrong.

Lesson: do not extrapolate per-hand cost from a contended machine, and
do not kill an instrument on an unmeasured extrapolation. Instrument C
should have been left to finish.
