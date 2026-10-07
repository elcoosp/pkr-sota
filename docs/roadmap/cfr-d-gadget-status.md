# CFR-D gadget — WORKING (2026-10-07)

**Design:** `cfr-d-gadget-design.md`.

## Status: functional end-to-end, verified

The CFR-D reach-subgame gadget is implemented, wired, and tested on the
vector solver, and **verified to fire on a real checkpoint**.

### Components (all in `crates/pkr-subgame/src/vector.rs` unless noted)

| Item | Role |
|---|---|
| `Node::Const { vals_p0 }` + `const_cfv` | fixed-value terminal |
| `Node::TermP1 { cbv }` + `cbv_cfv` | gadget Terminate leaf |
| `Solver::install_gadget(cbv)` | wraps root in P1 `[Terminate, Follow]` |
| `Solver::p0_root()` / `p0_root_mut()` | route P0 reads to Follow |
| `has_gadget()`, `gadget_terminate_prob()` | diagnostics |
| `p1_cbv_per_hand(cfg)` (`lib.rs`) | P1 counterfactual value vs blueprint |
| `solve_root_gadgetted(...)` (`vsearch.rs`) | the driver |
| `--search-gadget` | CLI (lbr_eval, tournament) |

### Verified

- **Unit tests:** `const_cfv_matches_manual_for_both_traversers`,
  `cbv_cfv_matches_manual_for_both_traversers`,
  `install_gadget_wraps_root_and_p0_reads_follow`,
  `gadget_solve_exploitability_bounded`,
  `gadget_br_le_blueprint_br` (P1's BR ≥ the Terminate floor),
  `solve_root_gadgetted_runs`.
- **Real checkpoint:** `lbr_eval --search-gadget` on the V5 checkpoint
  reports `gadget_solves=19` and a changed LBR (−1100 vs −1307
  gadget-off). The gadget path executes and alters play.

### The cbv is practical

`p1_cbv_per_hand` uses the sampled tree, whose deal count is
`|p0_range| × |p1_range|`. In production our range is a **point mass**,
so this is `1 × 1326 = 1326` deals — affordable. (A full 1326×1326
range would blow up; not our production case.)

### Known limitation

**OOP-acting only.** `solve_root_gadgetted` returns `None` when the
acting player is IP (solver P0 ≠ actor), falling back to the raw solve.
Wiring the IP gadget is symmetric but not yet done; a mirror of the
state would handle it, as `SubgameHandle::mirror_to_seat0` does for the
sampled path.

### Safety

The gadget gives the acting player's opponent a per-hand floor at the
blueprint's continuation value. Independent of the ranges used — the
theorem-backed bound, unlike `--search-bound` (heuristic, in-sample).
This is the mechanism suitable for a real external opponent.
