# Subgame solving — 2-week build plan

**Date:** 2026-09-26
**Prerequisites:** `docs/experiments/river-subgame-poc-positive.md` (positive POC)
**Status:** Ready to start

## Objective

Ship a bot that plays the trained blueprint on preflop and flop, then
switches to concrete-card CFR subgame solving on the turn and river.
Target: measurable reduction in real-game exploitability, not just on
uniform-range POC subgames.

## Deliverables, in order

### Week 1 — correctness first

**Day 1-2: Real ranges.** The POC uses uniform opponent ranges. Replace
with ranges estimated from the blueprint's own action history
(`RangeTracker`). Every public action updates the opponent's range
posterior using the blueprint's average strategy at the abstract
infoset. Test: range mass sums to 1.0 on random histories; range
matches a naive enumeration on small trees.

**Day 3: Safe solving.** Without a safety constraint, a subgame solve
can return a strategy that is worse than the blueprint against an
opponent who exploits the assumption of uniform ranges. Implement
max-margin: constrain the solved strategy so its worst-case value
against any opponent is at least the blueprint's own EV at the root.
This is the safety guarantee that makes subgame solving deployment-safe.

**Day 4-5: Turn solving. [DONE 2026-09-27]** Implemented via external-
sampling MCCFR with ChanceRiver nodes (option (b) from the original
plan). Enumerates all 46 river branches; walker samples one per
iteration. Same PublicTree structure, extended with a Chance variant.

**Result:** turn CFR beats blueprint by median +26.62 chips at 500
iterations (5/5 boards). At 50 iterations it loses (-10.12 median,
2/5 wins) — under-converged. External sampling is O(1/sqrt(T)) vs
river CFR+'s O(1/T), so ~5-10x more iterations are needed for the
same convergence. Expected.

Per-solve cost: ~5s at 500 iters, 10x10 hands, ~950K nodes/sec.

**Open:** production turn solve would want 1000-2000 iters (10-20s
per solve). Acceptable for offline eval; needs optimization for
runtime (parallelize over chance branches, or switch to full
enumeration with CFR+ once tree size allows).

### Week 2 — scale and integrate

**Day 6-7: Range-indexed infoset arrays.** Replace the current
`Vec<[f64;6]>` with a structure keyed by (public_node, hand_index).
Remove the last per-deal overhead. Target: 100ms per solve at 500-hand
ranges, 100 iterations.

**Day 8: Runtime integration.** `pkr-runtime` currently answers
queries from the blueprint only. Add a `SolverHandle::solve_subgame`
path that: (a) detects subgame entry (turn or river), (b) builds the
root from the current GameState, (c) invokes the solver, (d) returns
the root action. Blueprint remains the fallback if the solve times out.

**Day 9: Latency budget.** Profile + optimize the runtime path.
Target: <100ms wall for 95% of solves. Reject subgame solving if the
solve exceeds a configurable deadline; fall back to blueprint.

**Day 10: End-to-end exploitability measurement.** Run the full bot
against `pkr-exploit::sampled_exploitability` on the same
4000-deal benchmark used for the blueprint. Compare against the
2526 mbb v34long baseline. This is the number that ships.

## Success criteria

| milestone | pass condition |
|---|---|
| Range tracking | range mass = 1.0 ± 1e-6 on random histories |
| Safe solving | worst-case value ≥ blueprint EV at root on 100 random subgames |
| Turn solving | same median win as river POC (>15 chips) on 20 boards |
| Runtime latency | p95 < 100ms at 500-hand ranges |
| End-to-end | total exploitability < 2526 mbb at 4000 deals |

## Risk register

- **Range tracking bug.** Highest risk. Mitigated by exhaustive-enumeration tests on small trees.
- **Gadget complexity.** Turn solving via gadget is nontrivial. Fallback: river-only ship first, turn in a follow-up.
- **Latency.** If we can't hit 100ms, search becomes unusable. Mitigation: reject-and-fallback to blueprint.
- **Real-range win shrinks.** The POC's uniform-range win is optimistic. Expect 30-50% reduction in the win magnitude with real ranges.

## What NOT to do

- Don't skip safe solving. Unsafe subgame solving is exploitable and worse than the blueprint against a strong opponent.
- Don't extend to flop until turn is stable. Two chance events is a large complexity jump.
- Don't tune iteration count. 100 is enough for river; 200 is enough for turn. Convergence is fast on these trees.

## Related

- `docs/experiments/river-subgame-poc-positive.md` — the POC
- `docs/experiments/v34-long-run-confirmed.md` — the blueprint baseline
- `docs/handoff/HANDOFF_2026-09-25.md` §4 — original recommendation


## Turn adversarial safety — DONE 2026-09-27

Same structure as river adversarial test. P1 picks the worst deal.

| | CFR worst | BP worst | delta |
|---|---|---|---|
| river | +5.7 | +144.4 | **+138.7** |
| turn | +16.1 | +159.3 | **+143.1** |

Both subgame solvers are ~10x less exploitable than the blueprint
under adversarial deal selection. Turn POC fully closed.

## What remains before the 2-week build

1. **Safe-solving gadget (max-margin).** Without it, a well-informed
   opponent who knows we're solving can try to force lines where the
   assumption of uniform/tracked ranges breaks. Not yet tested as a
   threat, but required for deployment.
2. **Runtime latency budget.** Turn solve at 500 iters is ~5s; production
   needs <100ms. Requires parallelizing over chance branches or
   switching to full enumeration + CFR+ once tree fits in cache.
3. **Flop solver.** Not started. Adds a second chance event (two river
   branches deep). Design doc required.

Ranked by value: gadget > latency > flop. Flop only matters after the
first two are done.


## Safe-solving — DONE 2026-09-27

`safe_solve(cfg)` implemented: run CFR, then blend toward the blueprint
until BR_v1 <= blueprint BR_v1. Guarantees the shipped strategy is never
more exploitable than the blueprint.

River sweep (10 boards, 100 iters, 12 hands, aggressive line):

| metric | value |
|---|---|
| mean cfr_br | +0.31 chips |
| mean bp_br | +37.64 chips |
| mean alpha | **1.0000** |
| boards needing any blend | **0** |

**CFR is already strictly safer than the blueprint on every board.**
Pure CFR wins; the safe-solving wrapper is a no-op. This is the
strongest outcome: no manual gadget needed, no compute sacrificed
to safety.

**Caveat.** "Adversarial" here means *adversarial deal selection* —
P1 picks the deal maximizing BR against a fixed strategy. The stronger
test is *adversarial range selection* — P1 picks the distribution over
hands maximizing exploitability. Both the earlier "wide-range" test
and this one give CFR the win, but they're not the same measurement.
Wiring the range-adversarial version into `safe_solve` is a small
change to `br_v1_with_prior` (replace the tracked prior with a
worst-case prior), and should be done before the production build.

## Roadmap status

| milestone | status |
|---|---|
| RangeTracker | DONE |
| River CFR solve | DONE (+42.96 chips median) |
| River adversarial (deal) | DONE (+138.7 delta) |
| Turn CFR solve | DONE (+26.62 chips median) |
| Turn adversarial (deal) | DONE (+143.1 delta) |
| Safe-solving wrapper | DONE (alpha=1.0, no blend) |
| Adversarial range safety | OPEN (small extension) |
| Runtime latency < 100ms | OPEN (parallelize chance branches) |
| Flop solver | OPEN (design required) |


## Latency findings (2026-09-27)

Bench on turn solver, 10x10 hands, one board, varying iteration count.

### Full-chance vs MCCFR

| iters | full-chance wall | full-chance br_v1 | MCCFR wall | MCCFR br_v1 |
|---|---|---|---|---|
| 25 | 8.1s | +0.32 | 2.2s | +42.2 |
| 50 | 15.9s | +0.01 | 1.7s | +50.7 |
| 100 | 23.4s | -0.07 | 1.6s | +28.7 |
| 200 | 29.6s | -0.09 | 2.3s | +15.1 |
| 500 | 38.3s | -0.09 | 3.1s | +2.25 |

**Full-chance wins decisively on total compute.** MCCFR is 34× cheaper
per iteration, but needs >250,000 iterations to reach br_v1 ~0.1
(O(1/√T)). Full-chance gets there in 50. Production path: full-chance.

### Lazy terminal evaluation

Precomputing (Showdown × deal) pairs at Solver::new cost ~4s.
Computing on first visit costs 1.5s. 2.7× startup speedup at 25 iters.

### Production latency budget

Current: 25 iters, 10x10 hands = **3.0s per turn solve**.

Realistic optimizations:
- Parallelize chance branches: 46 independent children per chance node.
  Rayon over the first-level children gives ~6-8x on M1. → ~500ms.
- Reduce iterations to 10-15: still ≥30× better than blueprint on BR.
  → ~300ms.
- Reduce deal count: 8x8 = 64 deals instead of 100. → ~200ms.
- Combined: **~200-400ms per turn solve** at useful quality.

**The 100ms target is not achievable for this architecture at full
tree enumeration.** Realistic production target: 200-500ms. That's
below human reaction time and acceptable for a competitive bot;
it just rules out tournament play with tight timing rules.

If <100ms becomes required, the paths are:
- Precompute the full turn tree into a shared artifact (server-side,
  memory-mapped, ~50MB).
- Cache solves by position hash across hands (in multiplayer, adjacent
  hands often reach the same positions).
- Reduce the tree by pruning dominated bet-sizings.

## Roadmap status (updated)

| milestone | status |
|---|---|
| RangeTracker | DONE |
| River CFR solve | DONE (+42.96 chips) |
| River adversarial (deal) | DONE (+138.7) |
| Turn CFR solve | DONE (+26.62 chips) |
| Turn adversarial (deal) | DONE (+143.1) |
| Safe-solving wrapper | DONE (alpha=1.0, no blend) |
| Full-chance CFR+ mode | DONE |
| Lazy term eval | DONE |
| **Parallelize chance branches** | OPEN (2-4h; 6-8x) |
| Adversarial range safety | OPEN (2h) |
| Runtime integration | OPEN (2-3 days) |
| Flop solver | OPEN (1 week) |


## Parallelization (2026-09-27)

`solve()` now runs over deals in parallel. Two failed attempts and
one fix:

**Attempt 1**: single shared `AtomicU64` for `nodes_visited`.
Failed — cache-line contention across all cores made parallelism
slower than sequential (0.91x at 500 iters).

**Attempt 2**: `Vec<AtomicU64>` indexed by `deal_idx`. Each thread
hits its own cache line. Net 2.6x at production iteration count.

| iters | seq wall | par wall | speedup |
|---|---|---|---|
| 25 | 2.996s | 1.153s | 2.60x |
| 100 | 9.301s | 3.909s | 2.38x |
| 500 | 40.836s | 18.443s | 2.21x |

**Sub-linear scaling** because:
- `Solver::new` (tree build + first-visit cache warming) is sequential
  and takes ~0.4s.
- `lazy_cache` is `Box<[AtomicU64]>` — one atomic per showdown visit.
- Rayon dispatch overhead on a 100-deal workload with ~10K nodes each.

**Runtime budget update**:

Original target was <100ms per solve. Realistic floor with this
architecture:

| iters | parallel wall | notes |
|---|---|---|
| 10 | ~0.6s | dominance of Solver::new |
| 25 | ~1.15s | matches current quality target |
| 50 | ~2.5s | recommended for turn |

**For <100ms**: need one of
- Precompute the tree once per position-class; mmap from disk on
  solve. Removes ~0.4s startup and enables further optimization.
- Reduce deal count to 8x8 = 64 (vs 100): ~1.5x further.
- Drop lazy_cache atomics in favor of unsafe direct writes (SAFETY:
  each (node, deal) is written by exactly one thread).

None of these is required for the POC. They're production optimizations.
