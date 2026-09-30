# pkr-sota: Current Status

**Last updated:** 2026-09-30

This is the source of truth for what is actually implemented. For the
current session's work see `docs/HANDOFF_2026-09-30_audit.md`.

## Where the bot stands

**Tier: weak-bot.** Best pre-audit reading was ~2170 mbb on a
30M-iteration run (v38 seed 202), measured with an estimator that was
subsequently found to have a bug (see F1 below). For scale:
superhuman HUNL bots sit near 50 mbb, competent bots under 500 mbb.
The post-audit reading is not yet known — v41 is training.

Not competitive. May be useful as a sparring partner or as an
infrastructure base.

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
| F9 | no tournament harness | scaffold — `4f01dfe` |

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
- No LBR lower bound. The audit's F9 item 3.
- No suit isomorphism for flop/turn rebuilds. Blocks F4's turn half.
- No bot binary. `RuntimeSession` is the API a bot would use.

## Reference

- `docs/HANDOFF_2026-09-30_audit.md` — what this session changed.
- `docs/experiments/post-audit-invalidation.md` — what F6 invalidated.
- `docs/experiments/range-aware-solving-poc.md` — the subgame work.
- `docs/roadmap/post-ab-plan.md` — the production-grade checklist.
