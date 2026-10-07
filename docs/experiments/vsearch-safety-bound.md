# Vector-search safety bound (2026-10-06/07)

**Status:** implemented, wired, smoke-tested. Running in the overnight job.

## Why

The 2026-10-06 measurement showed the raw vector search helps on
BOTH instruments (LBR −377, H2H +1.25 chips/deal). But the raw solve
has **no safety bound**: a best-responding opponent, or an opponent
playing an off-distribution strategy the tracker did not anticipate,
could still punish it. Every strong re-solving bot bounds the
re-solved strategy (Libratus, DeepStack, ReBeL all use a gadget or a
CFR-D formulation).

## What was implemented (heuristic root bound)

The vector-path analog of `safe_solve_root_p0_strategy` (the sampled
path's heuristic). New primitives in `crates/pkr-subgame/src/vector.rs`:

- `root_strategy_all()` — per-action, per-hand root probabilities.
- `set_root_strategy(&probs)` — overwrite the root's average strategy.
- `opp_br_value()` — the opponent's best-response value in chips
  against the tree's current strategy. **Correct for either root
  actor** (the earlier `p1_br_value` was OOP-rooted only; `spec_from_state`
  sets `root_actor` from the live actor, so IP-rooted solves needed the
  `p == 0` term).
- `blueprint_root_strategy(abs, tbl, sig, street, st)` — the blueprint's
  root strategy, per action per hand, mapped through the same
  `action_bucket` the tracker uses.
- `blend_root(a, b, alpha)` — convex combination.

And in `crates/pkr-runtime/src/vsearch.rs`:

- `solve_root_bounded(...)`:
  1. Solve the raw tree (with `iterate_budgeted`).
  2. Compute the opponent-BR value of the pure search root and of the
     pure blueprint root.
  3. If the search root is already at least as safe as the blueprint,
     keep it whole (alpha = 1).
  4. Else binary-search the largest alpha in [0,1] with
     `blended BR <= blueprint BR`, and set the root to that blend.
  Non-root nodes keep the search continuation — only the root decision
  is bounded.
- `iterate_budgeted` — wall-clock cap (`VSearchCfg::time_budget_ms`),
  checked every `BUDGET_CHECK_EVERY = 5` iterations.
- `VSearchHandle::with_bound(cfg, abs, tbl)` — constructor supplying the
  blueprint sources.

CLI: `--search-bound` and `--search-budget-ms N` on `lbr_eval` and
`tournament`.

Diagnostics (`PKR_SEARCH_DEBUG=1`):
`VSEARCH_BOUNDED_SOLVES`, `VSEARCH_BOUND_KEPT_SEARCH`,
`VSEARCH_BOUND_BLENDED`, `VSEARCH_BOUND_FELL_BACK`.

## Smoke evidence

River, 50 hands, V4 checkpoint, 30 iters:
`bounded: solves=8 kept_search=7 blended=1 fell_back=0` — the bound
ran, kept the search root when it was already safe, and blended once.
Turn+river, 20 hands: 6 s (≈0.3 s/hand → 20k hands ≈ 1.7 h).

## What this is NOT

- **Not the CFR-D gadget.** This is an in-sample heuristic: it bounds
  the opponent-BR against the *tracked* ranges, not against every
  possible opponent. The theorem-backed gadget (plan Task 4.4) is still
  the follow-up.
- **Root-only.** Deeper nodes are unbounded. That is the standard
  trade (Libratus bounds the re-solved root); a full-subtree CFR-D is
  strictly more work.
- **Not a strength loss claim.** The bound can only *reduce* the
  search's strength when it fires, in exchange for safety. Whether it
  costs measurable strength is exactly what the overnight `H2H` runs
  will show (compare instrument D at seeds 42–45 against the
  unbounded result from 2026-10-06).

## Recommended follow-up

1. Read the overnight results; if the bounded search still wins H2H,
   the bound is cheap and should ship.
2. CFR-D gadget (plan 4.4) to replace the heuristic with a theorem.
3. Per-side `tournament` flags so a direct bounded-vs-unbounded H2H
   ablation is expressible (currently both sides share the search
   config).
