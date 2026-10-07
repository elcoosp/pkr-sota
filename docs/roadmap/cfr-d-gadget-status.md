# CFR-D gadget — implementation status (2026-10-07, final)

**Design:** `cfr-d-gadget-design.md`.

## What is DONE and TESTED (`65b260c`, `fd9f708`)

**`Node::Const` / `const_cfv`** (`fd9f708`) — fixed-value terminal.

**Gadget construction** (`65b260c`), all in `crates/pkr-subgame/src/vector.rs`:

| Item | Purpose |
|---|---|
| `Node::TermP1 { cbv }` | gadget Terminate leaf; pays P1 `cbv[h]` |
| `cbv_cfv(...)` | its CFV for either traverser |
| `Solver::install_gadget(cbv)` | wraps the root in a P1 `[Terminate, Follow]` node |
| `Solver::p0_root()/p0_root_mut()` | route P0 reads to the Follow child |
| `Solver::has_gadget()`, `gadget_terminate_prob()` | diagnostics |

`root_strategy`, `root_strategy_all`, `set_root_strategy`,
`blueprint_root_strategy` all route through `p0_root()`, so a
gadgetted solver's P0 strategy reads correctly.

**Tests (20 pass):**
- `cbv_cfv_matches_manual_for_both_traversers`
- `install_gadget_wraps_root_and_p0_reads_follow`
- `gadget_solve_exploitability_bounded` — **end-to-end**: with `cbv =
  0`, P1's best response at the gadget root is ≥ 0 (P1 can always take
  the zero-value Terminate). This is the gadget's defining property,
  verified.

`p1_cbv_per_hand(cfg)` (`lib.rs`) computes P1's cbv from the sampled
tree — correct, but see below.

## The one remaining piece: a practical cbv source

`p1_cbv_per_hand` uses the **sampled** `Solver`, whose tree is built
over `p0_range.hands × p1_range.hands` deals. With the vector path's
**full 1326-hand ranges** that is ~1.6 M deals — it blows up. It is
correct but impractical for a real solve.

A practical cbv needs a **vector-based** blueprint walk. That requires
the abstract infoset history at every vector-tree node, i.e. replicating
`GameState::history_signature_v3/v4/v5` inside the solver (the vector
tree is history-free; it tracks only per-street `raises`, not
`total_raises`, `last_was_bet`, or `last_bet_fraction_bucket`).

**Why not rushed:** a byte-mismatch in that replication makes every
node's blueprint hash wrong → garbage cbv → a **false safety
guarantee**, the one failure mode a safety feature must not have.

### Exact plan for the cbv source

1. Thread `total_raises: u8`, `actions_this_street: u8`,
   `last_was_bet: bool`, `last_bet_to_call_over_pot: f32` through `St`
   (it already has `n_act`, `raises`, `last_raise`; add the rest).
2. Add `sig_state: &mut SigState` to `build`, mirroring `board`.
3. **Verification test (mandatory):** replay a scripted line, asserting
   the solver's per-node signature equals
   `GameState::infoset_signature_into` byte-for-byte. Without this the
   step is unverifiable.
4. `blueprint_value_p1_vector(...)`: expectation walk, per-node
   `get_infoset_hash(hole, board, sig, street)` → `cbv[h]`.
5. Wire `solve_root_gadgetted` in `vsearch.rs`; `--search-gadget` flag.

Estimated: ~half a day, gated on step 3's byte-match test.

## Interim safety (shipped)

`--search-bound` (heuristic root blend) is the shipped bound. It is
in-sample; adequate for offline measurement, **NOT for a real external
opponent (Slumbot)**. The gadget mechanism is now ready for when the
cbv source lands.
