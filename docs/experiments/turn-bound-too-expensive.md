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
