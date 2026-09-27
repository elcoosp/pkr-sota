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
