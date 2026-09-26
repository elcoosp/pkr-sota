# River subgame solving POC — POSITIVE

**Date:** 2026-09-26
**Status:** Positive. Concrete-card CFR on a river subgame beats the trained blueprint by a median 9.6 chips (59% exploitability reduction) across 20 boards.

## Headline numbers

20 random boards, disjoint uniform ranges (12 hands each), 100 CFR iterations:

| metric | value |
|---|---|
| wins / total | 19/20 (95%) |
| median delta (BP - CFR) | +9.63 chips |
| median ratio (CFR / BP) | 0.414 |
| mean ratio | 0.440 |
| max delta | +36.46 chips |
| min delta | -1.99 chips |

## Method

- Root: 200-chip stacks, pot=4 at river after limp-check preflop + check-check flop + check-check turn.
- P0 range: uniform over 12 hands from cards 0..26. P1 range: uniform over 12 hands from 26..52.
- CFR: 100 iterations of vanilla CFR+ with linear averaging on the concrete river betting tree.
- Baseline: v34long 100M-iteration blueprint, evaluated at the same subgame with the same ranges.
- Measurement: P1 BR value against each P0 strategy in the concrete subgame.

## Implementation

New crate `crates/pkr-subgame/` (~450 lines):

- `lib.rs` — run_poc, cfr_solve_p0, CfrState::walk, compute_br_v1, blueprint_p0_strategy.
- `tests/river_poc.rs` — CFR vs uniform baseline.
- `tests/blueprint_compare.rs` — single-board CFR vs blueprint.
- `tests/blueprint_sweep.rs` — 20-board aggregate.

Key design: concrete (hole, node) infoset keys with no abstraction; pre-computed deal list reused across CFR iterations; cached hand ranks per deal; foldhash for regret maps.

## Performance

| version | 200-iter solve wall | notes |
|---|---|---|
| v0 | 393s | naive: HashMap + per-terminal evaluator + clone per deal |
| v1 | 6.1s | + foldhash + rank cache |

64x speedup, bit-identical results. Per-solve cost at 100 iters / 12-hand ranges: ~6s.

For 100ms production latency at realistic 500-hand ranges we need another 20-50x. Achievable via range-indexed infoset arrays (5-10x), parallel CFR over deal tree (3-4x), warm-start from blueprint (2-3x).

## Caveats

1. **Uniform ranges.** Real ranges are tighter. The 59% median reduction will shrink — likely to 20-40% — once ranges come from blueprint history.
2. **No safe-solving constraint.** Production needs max-margin or CFRD gadget to avoid exploitation by opponents who deviate.
3. **River only.** Turn adds one chance node; flop adds two. Solvable but requires gadget machinery.
4. **Single action history tested.** All 20 boards use the check-check-check line.

None of these undermine the core finding: at the river, concrete-card CFR dominates the abstracted blueprint.

## Decision

Proceed with full build: 3-4 weeks for river + turn + flop, safe solving, runtime integration.

## Related

- `docs/handoff/HANDOFF_2026-09-25.md` §4 (highest-leverage future work)
- `docs/experiments/v34-long-run-confirmed.md` (the blueprint we beat)
- `docs/experiments/variance-reduction-negative.md` (why eval variance is hard)
