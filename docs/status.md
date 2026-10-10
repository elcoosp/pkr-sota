# pkr-sota: Current Status

**Last updated:** 2026-10-10

This is the source of truth for what is actually implemented.

## Where the bot stands

**Tier: weak-bot.** LBR (local best response) lower bound on the
100M-iteration K=6 checkpoints:

| checkpoint | LBR mbb/hand | exploiter |
|---|---|---|
| v4 (V4 sig) | 821 (default) / 997 (samples=2000) / 227 (in-tree only) | LBR |
| v5 (V5 sig) | -2 (default) / 183 (samples=2000) / 29 (in-tree only) | LBR |

Scale: superhuman HUNL bots ~50 mbb; competent < 500 mbb. v4 is weak;
v5 is HARDER to exploit by greedy LBR but only marginally ahead in
self-play. Not competitive.

**Caveats that matter:**
- LBR is ONE greedy exploiter and has misled three times. The honest
  cross-process H2H (examples/v4v5_h2h.rs, CRN) shows V4 vs V5 are
  EQUAL in self-play (10k x 4 seeds, all |t|<1; V4 marginally ahead).
- In-sample BR (the trainer's own number) is NOT a strength measure;
  it fell 3796 -> 1707 -> 1222 as deals went 5k -> 20k -> 40k.

## K=7 action abstraction: SHIPPED as the new base (2026-10-10)

`NUM_ACTION_BUCKETS` 6 -> 7; `BET_SIZINGS` [0.5,1,2] -> [0.5,1,2,3];
bucket 6 = jam. Checkpoint-invalidating by design.

Baseline finished 100M iters in 19962s (5009.6 it/s, 4 threads, seed 42,
cap 60M). **Decision: SHIP.**

| metric | K=6 V5 | K=7 baseline | gate |
|---|---:|---:|---|
| LBR, 20k seed 42 | 183 ± 84 mbb | **-163 ± 104 mbb** | ✅ |
| H2H search-vs-bp seed 42 | — | **+0.958 chips, t=+5.28** | ✅ |
| H2H search-vs-bp seed 43 | — | **+1.279 chips, t=+7.02** | ✅ |

Both plan §3 gates cleared (LBR ≥ 2·SE, H2H > 2·SE). K=7 is the base;
K=6 checkpoints remain on disk but no longer the shipping config.
Full write-up: `docs/experiments/k7-decision-2026-10-10.md`.
K=6 and K=7 cannot meet in `tournament` or `lbr_eval`: stride + infoset
key both differ. See `docs/experiments/k6-vs-k7-incomparable.md`.

## Search (runtime)

- **Vector river search HELPS.** H2H vs blueprint on V5, 4 independent
  seeds: +1.18/+0.82/+0.97/+1.16 chips/deal (all t>4). On V4: LBR
  821->444, H2H +1.25.
- **Shipped safe config = the heuristic BOUND** (`--search-bound`;
  live gate `PKR_VSEARCH=1 PKR_VSEARCH_BOUND=1`). Bound safety PROVEN
  (final_br <= blueprint_br). Deadline now enforced in BR walks
  (Phase 4.5, 2026-10-10).
- **CFR-D gadget: SAMPLED proven** (bp_br 9.56 vs gadget_br 0.63);
  **VECTOR gadget PROVEN safe** (2026-10-09; EV cbv; worst delta -0.94
  over 4 subgames).

## What works

- **Training pipeline.** Precompute -> train -> export -> load -> query.
  Verified by `./smoke.sh` and the golden-hash CI check.
- **Runtime lookup.** `SolverHandle::get_advice_fast` p50 ~ 42ns,
  p99 ~ 84ns on M1 against v34long.
- **Subgame solving.** River-only via `RuntimeSession`. Turn is
  flat-to-negative at both 10 and 50 inner iterations; ship river-only.
- **Live bot binary.** `crates/pkr-runtime/src/bin/pkr-bot-pipe.rs`
  reads a JSON line, emits one action incr. Uses the same code path as
  tournament / arena / LBR.
- **Shadow state (Phase 1.1).** `crates/pkr-runtime/src/shadow.rs`
  keeps an in-tree view advanced by translated actions; blueprint hash
  and RangeTracker use it. `PKR_SHADOW=1`.
- **Stack depth param (Phase 0.1).** `PKR_STACK_BB` (default 100).
  Fingerprinted via `stack_bb_div2`, so a 100bb checkpoint refuses to
  load into a 200bb run.
- **Slumbot client (Phase 0.4).** `scripts/slumbot_client.py`. Wire
  format UNVERIFIED against `sample_api.py`; no live run yet.
- **Deterministic training.** Same seed + inputs -> identical
  `blueprint.bin`. See `docs/experiments/archive/training-nondeterminism.md`.

## What's broken or unknown (audit 2026-09-30)

Nine findings from an external audit. Status:

| # | description | state |
|---|---|---|
| F1 | estimator ignored opponent reach | **fixed** - `e43c74f` |
| F2 | config not reproducible | **fixed** - `72ad117` |
| F3 | infoset key can't see bet/pot size | V3-minimal shipped; V5 opt-in |
| F4 | abstraction noise-dominated | histogram module landed; buckets not rebuilt |
| F5 | regret floor + averaging site | flags added; Leduc grid run (eps win) |
| F6 | jam illegal + raise clamp | **fixed** - `e73e39c` |
| F7 | eager zero allocation | **fixed** - `839ed47` |
| F8 | purify + fallback bug | **fixed** - `4564848` |
| F9 | no tournament harness | **done** - tournament + LBR + cross-process H2H |

## What's running

`outputs/k7-baseline/` - K=7 baseline train (PID 59819, 100M iters,
seed 42, cap 60M, 4 threads). ETA ~16:52. A search-vs-blueprint H2H
(PID 9576) is queued behind it. See `docs/roadmap/queue-2026-10-10.md`.

Next queued: NLHE epsilon A/B (`/tmp/eps-ab.sh`), gated on the K=7
decision. Leduc grid (2026-10-10) says eps=0.002 ~35% better than 0.01;
see `docs/experiments/leduc-cfr-grid-2026-10-10.md`.

## What isn't there

- No external-strength evaluation. Nothing has played Slumbot or any
  public API. Slumbot client written; wire format unverified.
- 200bb build not launched (stack param + fingerprint exist).
- LBR lower bound: `lbr_eval` (JSON line). One greedy exploiter; has
  misled 3x. Use `--samples 2000` and cross-check with H2H.
- No suit isomorphism for flop/turn rebuilds. Task 2.3a attempt
  REVERTED (test caught over-identification); see
  `docs/roadmap/PLAN_STATUS_2026-10-10.md`.
- Turn-time search is offline-only (~45s before Phase 4.5 fix; now
  bounded, not yet measured live).
- Depth-limited flop search not started.

## Reference

- `docs/experiments/VERIFIED_FINDINGS.md` - authoritative numbers.
- `docs/roadmap/PLAN_STATUS_2026-10-10.md` - plan progress.
- `docs/experiments/k6-vs-k7-incomparable.md` - K=6/K=7 relationship.
- `docs/INDEX.md` - full docs index.
