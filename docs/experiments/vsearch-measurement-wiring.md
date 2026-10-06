# vsearch measurement wiring (2026-10-06, session 2)

**Status:** wiring landed; measurement QUEUED (waiting on the V4/V5 A/B).

## What this session did

Handoff §4 said: *"Measure whether vector search helps. Two instruments,
BOTH required: paired LBR and H2H. Both are wired to `SubgameHandle`
(the parked sampled solver), NOT vsearch — that is the integration gap
for the next session."*

This session closed that gap.

### The problem

`lbr_eval` and `tournament` each built a `SubgameHandle` (sampled-hand
solver, parked after `search-built-negative-h2h.md`) and hard-wired it
into their walkers. `RuntimeSession::advise` already routes through
`vsearch::solve_root` when `PKR_VSEARCH=1`, but the two offline
instruments never went through `RuntimeSession`.

So there was NO way to measure the vector solver's strength. LBR and
H2H both measured the wrong solver.

### The fix (4 commits)

1. **`c8fa4a1`... no, `a662fa4`** — added `SearchHandle` trait in
   `crates/pkr-runtime/src/subgame.rs`:
   ```rust
   pub trait SearchHandle: Send + Sync {
       fn decide(&self, state, our_hole, opp_range) -> Option<[f64; 6]>;
   }
   impl SearchHandle for SubgameHandle { ... }
   ```
   One name, two implementations. Object-safe so callers hold
   `&dyn SearchHandle` or `Box<dyn SearchHandle>`.

2. **`618ee1d`** — added `VSearchHandle` in `vsearch.rs` implementing
   the trait over the vector solver. Masks board overlap on both
   ranges, orders `[OOP, IP]` correctly, maps `Act` -> 6 buckets via
   `action_bucket`.

3. **`8e41682`** — switched `EvalContext.search` and both `tournament`
   signatures from `Option<&SubgameHandle>` to
   `Option<&dyn SearchHandle>`. The tournament bin now returns
   `Option<Box<dyn SearchHandle>>` from `mk_handle` and passes
   `.as_deref()`.

4. **`1cbc99e`** — added `--search-vector` to both `lbr_eval` and
   `tournament`. `VSearchCfg` gained `enabled_streets: [bool; 4]` with a
   street gate inside the impl (returns `None` immediately if the
   current street is disabled — no wasted solve).

5. **`8ca4026`** — three trait-level tests in `vsearch.rs`:
   * `vsearch_handle_through_trait` — full river solve via the trait,
     distribution sums to 1.
   * `vsearch_handle_returns_none_when_street_disabled`.
   * `vsearch_handle_returns_none_on_board_hole_overlap`.

6. **`fa8b9b1`** — `--search-streets` on the tournament bin, matching
   the flag already on `lbr_eval`.

All 19 runtime lib tests, 13 fuzz lib tests, 34 exploit lib tests pass.

## The queued measurement

`/tmp/vsearch-measure2.sh` (PID seen at 9017) polls for
`=== complete ===` in `/tmp/overnight-ab-result.txt`, waits 60s, then
runs four instruments against `outputs/ab-v4/train.ckpt` (the V4 arm of
the A/B), 20k hands, seed 42, 400 samples:

| # | What | Why |
|---|---|---|
| 0 | `lbr_eval --search-vector --search-streets n` (2000 hands) | plumbing sanity: hook installed but all streets disabled must match baseline |
| 1a | `lbr_eval` blueprint only | baseline LBR |
| 1b | `lbr_eval --search-vector --search-streets r --search-iters 100` | LBR with river vector search |
| 2 | `tournament --search-a --search-vector --search-streets r --search-iters 100` | H2H: search as one side vs blueprint |

Output: `outputs/vsearch-measurement/` and
`/tmp/vsearch-measure2-result.txt`.

River-only first, because the plan §4.5 says "River: full solve ...
budget 1-2 s" while turn needs 2-4 s/solve on 8 cores — at 20k deals
with turn search on, the measurement would run for days.

## Decision rule (from handoff §1 + §4)

Read `/tmp/vsearch-measure2-result.txt` when it ends with `ALL DONE`.

* **Plumbing sanity (#0) must ≈ #1a** (2000 vs 20000 hands, so ±3-4 SE).
  If it diverges, the hook is perturbing the walker and everything below
  is void.
* **Ship the search iff:**
  * LBR `#1b < #1a - 2*SE_combined` (improvement in LBR), **AND**
  * H2H `#2 mean_diff > +2*SE` (search wins head-to-head, as A).
* **If LBR improves but H2H is flat/negative:** do NOT ship. This is the
  exact failure mode that killed the sampled solver
  (`search-built-negative-h2h.md`): LBR alone is not a strength measure.
* **If H2H improves but LBR is flat:** marginal; consider a second seed
  before shipping.
* **If both flat:** the vector solver at these settings does not help;
  next lever is 2.3 (distribution-aware buckets).

## What is NOT in this measurement

* **Turn search.** The cost of running turn subgames at 20k deals was
  prohibitive for the first measurement. If river helps, a turn-only
  follow-up should be cheaper per solve than the full [t, r].
* **CFR-D gadget (Task 4.4).** The gadget in `pkr-subgame` is on the
  sampled solver's path, not the vector solver's. `VSearchHandle`
  returns the RAW vector solve, no safety bound — exactly the sampled
  solver's original sin. If the vector solve helps here, the next
  session MUST add a gadget before shipping it to a real opponent.
* **200 bb.** Slumbot's stack size (plan §0.1). Separate build.
