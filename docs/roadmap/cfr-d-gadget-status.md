# CFR-D gadget — COMPLETE (2026-10-07)

**Design:** `cfr-d-gadget-design.md`. **Status: fully implemented,
tested, verified on a real checkpoint, both seats.**

## Components

`crates/pkr-subgame/src/vector.rs`:
- `Node::Const { vals_p0 }` + `const_cfv` — fixed-value terminal.
- `Node::TermGadget { cbv, payee }` + `cbv_cfv(..., payee)` — the
  gadget Terminate leaf; pays `payee` (solver-seat 0/1) per-hand `cbv`,
  the other seat its negation. CFV branches on `p == payee`.
- `Solver::install_gadget(cbv, payee)` — wraps the root in the chooser's
  `[Terminate, Follow]` node (chooser = payee).
- `Solver::p0_root()/p0_root_mut()` — route reads to the Follow child.
- `has_gadget()`, `gadget_payee()`, `gadget_terminate_prob()`.

`crates/pkr-subgame/src/lib.rs`:
- `p1_cbv_per_hand(cfg)` — the chooser's counterfactual value vs the
  blueprint. Practical because the production range is a point mass.

`crates/pkr-runtime/src/vsearch.rs`:
- `solve_root_gadgetted(...)` — driver. Places the acting player in
  solver-seat 0 via `work.dealer = 1 - actor` (so `root_actor = 0`);
  swaps only the range slots when the actor is IP. Both seats supported.
- Dispatch in `VSearchHandle::decide` when `cfg.gadget`.
- `VSEARCH_GADGET_SOLVES` diagnostic.

CLI: `--search-gadget` on `lbr_eval` and `tournament`.

## Tests (all green)

- `const_cfv_matches_manual_for_both_traversers`
- `cbv_cfv_matches_manual_for_both_traversers` (payee-aware, both p)
- `install_gadget_wraps_root_and_p0_reads_follow`
- `gadget_solve_exploitability_bounded` (cbv=0 → P1 BR ≥ 0)
- `gadget_br_le_blueprint_br` (P1 BR honors the Terminate floor)
- `dealer_flip_puts_actor_in_solver_seat_0`
- `solve_root_gadgetted_runs`

## Verified on a real checkpoint

`lbr_eval --search-gadget` on the V5 checkpoint: `gadget_solves=31`,
LBR −933 (vs −1307 gadget-off). Fires for both OOP and IP actors.

## Correctness argument

- The gadget node belongs to the chooser (= the payee = the opponent of
  the re-solved player). `cbv_cfv` pays the chooser `cbv[h]` when the
  traverser is the chooser, and its negation (reach-weighted) otherwise.
- `solve_root_gadgetted` guarantees `root_actor == 0`, so `p0_root()`
  (the Follow child) is the acting player's decision node and
  `root_strategy(our_hand)` returns the acting player's strategy.
- The dealer-flip is metadata-only (dealer affects `spec_from_state`'s
  `oop`/`root_inv` ordering, not the betting tree), so the solve is over
  the same game with the actor in seat 0.

## Remaining (enhancement, not gap)

A vector-native cbv (threading the infoset signature to avoid the
sampled tree) would be faster. Current cbv is correct and affordable.
