# pkr-sota: Current Status

**Last updated:** 2026-10-09

This is the source of truth for what is actually implemented.

## Where the bot stands

**Tier: weak-bot.** LBR (local best response) lower bound on the
100M-iteration checkpoints:

| checkpoint | LBR mbb/hand | exploiter |
|---|---|---|
| v4 (V4 sig) | 821 (default) / 997 (samples=2000) / 227 (in-tree only) | LBR |
| v5 (V5 sig) | -2 (default) / 183 (samples=2000) / 29 (in-tree only) | LBR |

Scale: superhuman HUNL bots ~50 mbb; competent < 500 mbb. So v4 is
weak; v5 is HARDER to exploit by greedy LBR but only marginally ahead
in self-play (see below). Not competitive.

**Caveats that matter:**
- LBR is ONE greedy exploiter and has misled three times. The honest
  cross-process H2H (examples/v4v5_h2h.rs, CRN) shows V4 vs V5 are
  EQUAL in self-play (10k x 4 seeds, all |t|<1; V4 marginally ahead).
- In-sample BR (the trainer's own number, ~6100 mbb) is NOT a strength
  measure.

## Search (runtime)

- **Vector river search HELPS.** H2H vs blueprint on V5, 4 independent
  seeds: +1.18/+0.82/+0.97/+1.16 chips/deal (all t>4). On V4: LBR
  821->444, H2H +1.25.
- **Shipped safe config = the heuristic BOUND** (`--search-bound`;
  live gate `PKR_VSEARCH=1 PKR_VSEARCH_BOUND=1`). Bound safety PROVEN
  (final_br <= blueprint_br).
- **CFR-D gadget: SAMPLED proven** (bp_br 9.56 vs gadget_br 0.63);
  **VECTOR gadget PROVEN safe** (2026-10-09; EV cbv; worst delta -0.94 over 4 subgames).
  For a real opponent use the sampled gadget (SubgameHandle).

## What works

- **Training pipeline.** Precompute → train → export → load →
  query. Verified end to end by `./smoke.sh` (10 iterations, k=8)
  and by the golden-hash CI check (`ci/scripts/golden-training.sh`,
  100k iterations, k=200).
- **Runtime lookup.** `SolverHandle::get_advice_fast` p50 ≈ 42ns,
  p99 ≈ 84ns on the M1 against the v34long blueprint. Batch variant
  `get_advice_batch` walks the sorted key table once.
- **Subgame solving.** River-only, integrated through
  `RuntimeSession`. Verified +2.40 chips/deal (t=6.04) at 20k paired
  deals across three checkpoints. Turn is flat-to-negative at both 10
  and 50 inner iterations — ship river-only.
- **Deterministic training.** Same seed + same inputs → identical
  `blueprint.bin` and `exploitability.csv` at any thread count.
  `train.ckpt` is byte-identical at 1 thread; the 4+ thread strategy-
  sum accumulation has a residual divergence documented in
  `docs/experiments/training-nondeterminism.md`, which does not
  affect the shipped artifacts.

## What's broken or unknown (audit 2026-09-30)

Nine findings from an external audit. Status:

| # | description | state |
|---|---|---|
| F1 | estimator ignored opponent reach | **fixed** — `e43c74f` |
| F2 | config not reproducible | **fixed** — `72ad117` |
| F3 | infoset key can't see bet/pot size | gated infrastructure, off |
| F4 | abstraction noise-dominated | function + plan, rebuild not launched |
| F5 | regret floor + averaging site | flags added, grid not run |
| F6 | jam illegal + raise clamp | **fixed** — `e73e39c` |
| F7 | eager zero allocation | **fixed** — `839ed47` |
| F8 | purify + fallback bug | **fixed** — `4564848` |
| F9 | no tournament harness | **done** — tournament + LBR + cross-process H2H (`examples/v4v5_h2h.rs`, CRN) |

The F1 fix changed the 1-deal reading from 8981 to 1748 mbb. Every
number published before 2026-09-30 was measured with that estimator.
See `docs/experiments/post-audit-invalidation.md`.

## What's running

`outputs/v41-post-f6/` — the first clean post-F1/F2/F6 training run.
30M iterations, seed 42, F2 experiment defaults, F6 game tree.

## What isn't there

- No real-game evaluation. Nothing has ever played a full game
  against a scripted opponent suite. `pkr-fuzz::tournament` exists
  but the scripted-bot wiring is untested end to end.
- LBR lower bound: `lbr_eval` (JSON line). NOTE: one greedy exploiter; it has misled 3x. Use `--samples 2000` for a stronger reading and cross-check with H2H.
- No suit isomorphism for flop/turn rebuilds. Blocks F4's turn half.
- No bot binary. `RuntimeSession` is the API a bot would use.

## Reference

- `docs/experiments/post-audit-invalidation.md` — what F6 invalidated.
- `docs/experiments/range-aware-solving-poc.md` — the subgame work.
- `docs/roadmap/post-ab-plan.md` — the production-grade checklist.
