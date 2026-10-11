# Phase 5 design: depth-limited flop search (2026-10-11)

**Status:** design only. Nothing below is implemented. This is the last
substantial plan item; Phase 6 (Slumbot) is network-gated, and the audit
work is otherwise closed (see `PLAN_STATUS_2026-10-10.md`).

## What the plan asks for (verbatim)

> **Depth-limited flop search** with Pluribus-style multiple continuation
> strategies (4 biased copies of blueprint: fold-/call-/raise-biased;
> `FOLD_BIASED/CALL_BIASED/RAISE_BIASED` + `bias_strategy` already exist
> in `pkr-subgame`). Leaf = end of flop betting; leaf value for each hand
> = blueprint EV with the opponent choosing among the 4 continuations.
> Needs blueprint EV tables per (hand class, public state) — compute by
> rollouts.

## What exists today

| Piece | Where | State |
|---|---|---|
| River vector solver | `crates/pkr-subgame/src/vector.rs` | production. arena-backed since M-31 |
| Turn solve (offline) | same; `PKR_VSEARCH_STREETS=tr` | 45 s/decision, not live |
| Gadget + bound + deadline | `vsearch.rs`, `vector.rs` | production |
| `bias_strategy` + 3 bias vectors | `crates/pkr-subgame/src/lib.rs:190-213` | exists, **no caller** |
| CFR-D gadget (sampled) | `crates/pkr-subgame/src/lib.rs` | production |
| Flop in the vector tree | `vector.rs::end_street` | enumerates the flop → turn → river, so a flop solve builds a tree of ~47×46 chance nodes |
| `riversolve.rs` | `crates/pkr-cfr/src/riversolve.rs` | **DO NOT WIRE** — its own header lists 4 correctness bugs |


## Why we can't just call the existing solver on the flop

The vector solver carries the full 1326-hand range through every node.
On the river it is fast (single chance layer; 0.6 s for 800 iters in the
Appendix-A reference). On the turn it is 45 s offline because the tree
expands 46 rivers x every betting node. On the **flop**, the tree
expands 47 x 46 = 2162 river boards per leaf. Empirically the flop
build alone would be minutes and the solve would hold GBs in `chance`
clones. This is why Pluribus (and every production HUNL bot since
2019) uses **depth-limited solving**: solve the flop betting tree only,
and at every leaf evaluate the value of "the rest of the hand" from a
precomputed table, not by playing it out.

## Design

### 1. Subgame = flop betting tree only

Same vector solver, but `end_street` on the flop returns a **leaf
value node** instead of a `Node::Chance`. `Node::LeafEv { values_p0:
Vec<f32> }` is the new variant (the existing `Node::Const` is close
but carries a pre-hand-indexed value used by the gadget; reuse only
if the semantics match exactly).

Flop decision tree: ~7 buckets per node, 3 raises max. Same shape the
turn tree has minus the chance layer, i.e. tens of milliseconds to
build.

### 2. Leaf value = worst case over 4 continuation strategies

Per the plan: at a flop leaf, the opponent picks the continuation
among 4 (blueprint + 3 biased copies) that MINIMISES our EV; we play
the best response. This is the "safe" aggregation Pluribus uses.

For each hand h:

    leaf_value[h] = min over k in {bp, fold-bias, call-bias, raise-bias}:
        ev_blueprint_vs_continuation_k[h, public_state]

We need an EV table `ev[h][public_state]` per continuation. Four tables.
These are built offline, not computed live.

### 3. Blueprint EV tables -- the actual work

`public_state` at a flop leaf is (canonical_board_3, pot_class,
stack_class). `hand_class` is the same abstraction bucket the blueprint
uses at the flop. The tabulation the plan asks for is:

    EV[(flop_bucket, pot_class, stack_class)] -> f32

The flop leaf does not need the exact 1326 hands; it needs the value of
the public abstract state. The subgame solver then aggregates back to
per-hand values by expanding every hand in that bucket.

**How to build the tables.** Two candidates:

**(A) Exhaustive flop rollout from the blueprint.** For every canonical
flop board (~1.29M) times the pot/stack classes, deal both hands
randomly against the blueprint and record chip EV per hand bucket.
Monte-Carlo: to get +-0.05 bb you need N ~= (sigma/0.05)^2 ~= 400
rollouts per (board, class). 1.29M x ~10 classes x 400 = ~5e9
blueprint walks. Not feasible on this machine.

**(B) Walk the trained blueprint's own tree.** The blueprint already
contains the strategy at every flop infoset. The value of "play out
from this flop leaf with the blueprint in both seats" is a full
traversal of the blueprint's turn+river subtree. Cost: one turn-solve
per flop leaf, i.e. the 45 s offline number times the leaf count.
Not feasible either.

**The honest conclusion** is that the leaf EV table at the plan's
implicit resolution (all flop boards) is too expensive. A production
flop search needs a MUCH coarser leaf bucketing (512-2048 flop
buckets, small pot/stack classes). That is a new abstraction
decision, not covered by the current plan.

## What this means for Phase 5

Phase 5 as written assumes the leaf EV table can be produced. This
recon says that assumption is untested and probably wrong at the
current abstraction resolution. The correct next step is NOT to
implement Phase 5; it is to **measure the cost of the leaf EV table on
a small slice**:

1. Pick one canonical flop board, one pot class.
2. Compute `leaf_value[h]` by the blueprint walk (B) for a single
   bucket and time it. Multiply by the target count.
3. If the extrapolated cost exceeds one training run, Phase 5 is out
   of scope on this hardware and should be replaced by either:
   - a **flop continuation-strategy lookup** (the blueprint's own
     strategy -- we already have it), or
   - a **flop subgame solve against a heavily coarsened leaf bucket**
     (a design decision, not a mechanical implementation).

## Concrete follow-up tasks (in order)

1. **Measure.** Time the TURN subtree build (4-card board, 46 rivers).
   Multiply by 47 to get the flop-leaf cost. **Do NOT actually build a
   flop tree** (3-card board): it is ~46x the turn tree, i.e. GB-scale,
   and (observed 2026-10-11) will OOM the machine mid-test. The turn
   build is a valid proxy because the flop tree is exactly 47 nested
   turn trees. ~30 lines in `crates/pkr-subgame/tests/` and a
   `cargo test --nocapture`. If the extrapolated flop cost is > 1 ms
   per leaf (it will be), stop here.
2. **Decide the leaf bucket resolution.** 200 flop buckets x ~10 pot
   classes x ~3 stack classes = 6000 leaf classes; or 2048 x 10 x 3 =
   60k. The flop bucket already exists; the pot/stack classes need
   choosing.
3. **Build the leaf EV table.** Only if step 1 says it is tolerable.
   Chunked, resumable, same pattern as `precompute turn`.
4. **Wire the leaf node into the vector solver.** `Node::LeafEv` +
   `spec_from_state` passes the table. Arena/index discipline as
   everywhere else in `vector.rs`.
5. **Test.** A flop solve must reproduce the blueprint's river values
   on a river-reachable leaf where the blueprint is ground truth. Then
   the plan's acceptance test (flop solve no more exploitable than the
   blueprint on a held-out flop set).

## Not recommended

- Reusing `riversolve.rs`. Its own header lists 4 correctness bugs and
  says "DO NOT WIRE".
- Solving the flop without a leaf table. The tree is too large.

## Summary

Phase 5 is a larger project than the plan implies, and its prerequisite
(leaf EV table at current abstraction resolution) is unproven. Do not
start it before the step-1 measurement. The realistic alternative for a
flop search sooner is a coarsened leaf bucket -- a strength-for-speed
trade that needs its own A/B.

## Why we can't just call the existing solver on the flop

The vector solver carries the full 1326-hand range through every node.
On the river it is fast (single chance layer; 0.6 s for 800 iters in the
Appendix-A reference). On the turn it is 45 s offline because the tree
expands 46 rivers x every betting node. On the **flop**, the tree
expands 47 x 46 = 2162 river boards per leaf. Empirically the flop
build alone would be minutes and the solve would hold GBs in `chance`
clones. This is why Pluribus (and every production HUNL bot since
2019) uses **depth-limited solving**: solve the flop betting tree only,
and at every leaf evaluate the value of "the rest of the hand" from a
precomputed table, not by playing it out.

## Design

### 1. Subgame = flop betting tree only

Same vector solver, but `end_street` on the flop returns a **leaf
value node** instead of a `Node::Chance`. `Node::LeafEv { values_p0:
Vec<f32> }` is the new variant (the existing `Node::Const` is close
but carries a pre-hand-indexed value used by the gadget; reuse only
if the semantics match exactly).

Flop decision tree: ~7 buckets per node, 3 raises max. Same shape the
turn tree has minus the chance layer, i.e. tens of milliseconds to
build.

### 2. Leaf value = worst case over 4 continuation strategies

Per the plan: at a flop leaf, the opponent picks the continuation
among 4 (blueprint + 3 biased copies) that MINIMISES our EV; we play
the best response. This is the "safe" aggregation Pluribus uses.

For each hand h:

    leaf_value[h] = min over k in {bp, fold-bias, call-bias, raise-bias}:
        ev_blueprint_vs_continuation_k[h, public_state]

We need an EV table `ev[h][public_state]` per continuation. Four tables.
These are built offline, not computed live.

### 3. Blueprint EV tables -- the actual work

`public_state` at a flop leaf is (canonical_board_3, pot_class,
stack_class). `hand_class` is the same abstraction bucket the blueprint
uses at the flop. The tabulation the plan asks for is:

    EV[(flop_bucket, pot_class, stack_class)] -> f32

The flop leaf does not need the exact 1326 hands; it needs the value of
the public abstract state. The subgame solver then aggregates back to
per-hand values by expanding every hand in that bucket.

**How to build the tables.** Two candidates:

**(A) Exhaustive flop rollout from the blueprint.** For every canonical
flop board (~1.29M) times the pot/stack classes, deal both hands
randomly against the blueprint and record chip EV per hand bucket.
Monte-Carlo: to get +-0.05 bb you need N ~= (sigma/0.05)^2 ~= 400
rollouts per (board, class). 1.29M x ~10 classes x 400 = ~5e9
blueprint walks. Not feasible on this machine.

**(B) Walk the trained blueprint's own tree.** The blueprint already
contains the strategy at every flop infoset. The value of "play out
from this flop leaf with the blueprint in both seats" is a full
traversal of the blueprint's turn+river subtree. Cost: one turn-solve
per flop leaf, i.e. the 45 s offline number times the leaf count.
Not feasible either.

**The honest conclusion** is that the leaf EV table at the plan's
implicit resolution (all flop boards) is too expensive. A production
flop search needs a MUCH coarser leaf bucketing (512-2048 flop
buckets, small pot/stack classes). That is a new abstraction
decision, not covered by the current plan.

## What this means for Phase 5

Phase 5 as written assumes the leaf EV table can be produced. This
recon says that assumption is untested and probably wrong at the
current abstraction resolution. The correct next step is NOT to
implement Phase 5; it is to **measure the cost of the leaf EV table on
a small slice**:

1. Pick one canonical flop board, one pot class.
2. Compute `leaf_value[h]` by the blueprint walk (B) for a single
   bucket and time it. Multiply by the target count.
3. If the extrapolated cost exceeds one training run, Phase 5 is out
   of scope on this hardware and should be replaced by either:
   - a **flop continuation-strategy lookup** (the blueprint's own
     strategy -- we already have it), or
   - a **flop subgame solve against a heavily coarsened leaf bucket**
     (a design decision, not a mechanical implementation).

## Concrete follow-up tasks (in order)

1. **Measure.** Time a `blueprint_p1_br`-style walk on ONE flop leaf
   at the current `flop_k = 200`. ~30 lines in
   `crates/pkr-subgame/tests/` and a `cargo test --nocapture`. If it
   is > 1 ms per leaf (it will be), stop here.
2. **Decide the leaf bucket resolution.** 200 flop buckets x ~10 pot
   classes x ~3 stack classes = 6000 leaf classes; or 2048 x 10 x 3 =
   60k. The flop bucket already exists; the pot/stack classes need
   choosing.
3. **Build the leaf EV table.** Only if step 1 says it is tolerable.
   Chunked, resumable, same pattern as `precompute turn`.
4. **Wire the leaf node into the vector solver.** `Node::LeafEv` +
   `spec_from_state` passes the table. Arena/index discipline as
   everywhere else in `vector.rs`.
5. **Test.** A flop solve must reproduce the blueprint's river values
   on a river-reachable leaf where the blueprint is ground truth. Then
   the plan's acceptance test (flop solve no more exploitable than the
   blueprint on a held-out flop set).

## Not recommended

- Reusing `riversolve.rs`. Its own header lists 4 correctness bugs and
  says "DO NOT WIRE".
- Solving the flop without a leaf table. The tree is too large.

## Summary

Phase 5 is a larger project than the plan implies, and its prerequisite
(leaf EV table at current abstraction resolution) is unproven. Do not
start it before the step-1 measurement. The realistic alternative for a
flop search sooner is a coarsened leaf bucket -- a strength-for-speed
trade that needs its own A/B.
