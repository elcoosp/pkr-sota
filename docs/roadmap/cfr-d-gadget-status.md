# CFR-D gadget — implementation status (2026-10-07)

**Design:** `cfr-d-gadget-design.md`.
**Status:** Stage 1 DONE + tested. Stages 2–4 specified, NOT implemented.

## Why staged, not rushed

The gadget is a **safety** feature. A subtly wrong CFR construction
produces a **false safety guarantee** — strictly worse than no gadget,
because it would be trusted. It was built in verifiable stages; Stage 1
is committed and tested; Stages 2–4 are specified for a focused session.

## Stage 1 — DONE (`fd9f708`)

- `Node::Const { vals_p0: Vec<f32> }` — terminal paying a per-hand value
  to P0, before opponent-reach weighting.
- `const_cfv(...)` — its CFV, same convention as `fold_cfv`/`show_cfv`.
- Arms in both `cfr` and `br`.
- Test `const_cfv_matches_manual_for_both_traversers` (p = 0 and 1).

17 `pkr-subgame` lib tests pass.

## The blocker for Stages 2–4

The vector solver is **history-free**. `build` threads `board` but not
the abstract betting history; `get_infoset_hash` needs it. The root hash
is supplied from OUTSIDE via `blueprint_root_strategy(..., history_sig)`.

The gadget needs the blueprint strategy at EVERY node, so the abstract
history must be threaded through the tree — and it must match
`infoset_signature_into` **exactly**, or every node's blueprint hash is
wrong and the "safety" bound is garbage. Getting that match right is the
careful part.

## Stage 2 — thread the abstract history (must match `infoset_signature_into`)

- `Solver::new` gains a `root_history: Vec<u8>` (default empty → current
  behaviour; avoids touching every `Spec` literal).
- `build(&self, st, board, hist: &mut Vec<u8>)` — `hist` mirrors
  `board`'s push/recurse/pop.
- `ActNode` gains `history: Vec<u8>`.
- **Critical:** the bytes pushed must equal what
  `GameState::infoset_signature_into` would produce for that public
  action sequence. Build a test that replays a scripted line in both the
  vector tree and a `GameState`, asserting the stored histories match
  byte-for-byte. Without that test, Stage 2 is unverifiable.

## Stage 3 — all-nodes blueprint strategy

`blueprint_strategy_all_nodes(...)` — for each `ActNode` × hand, the
6-bucket strategy from `get_infoset_hash(hole, board, node.history,
street)`. Template: `fill_blueprint_strat` in `pkr-subgame/src/lib.rs`.

## Stage 4 — the gadget

1. `p1_cbv_vs_blueprint(bp)` — P1 counterfactual-BR vs the fixed
   blueprint, one `br`-style walk.
2. Root: P1 `ActNode` `[Terminate, Follow]`; `Terminate` kid =
   `Node::Const { vals_p0: -cbv }`; `Follow` kid = the real root.
3. `solve_root_gadgetted(...)`; `--search-gadget` flag.

## Acceptance (port the sampled test)

`pkr-subgame/src/lib.rs` has an `#[ignore]`
`gadget_not_more_exploitable_than_blueprint`. Port it: the gadget
solve's exploitability vs a best-responding opponent must be ≤ the
blueprint's, same subgame.

## Interim safety

`--search-bound` (heuristic root blend, `vsearch-safety-bound.md`) is
the shipped bound. In-sample, NOT theorem-backed — fine for offline
measurement, NOT for a real external opponent. **Do not ship to Slumbot
without the gadget.**
