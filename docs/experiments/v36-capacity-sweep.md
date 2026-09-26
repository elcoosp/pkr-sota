# v36 — capacity sweep + iters-per-sync sweep

**Date:** 2026-09-26
**Status:** Capacity hypothesis NOT supported (provisional — 60M run still in progress). Sync sweep pending.

## TL;DR

The apparent 60M-vs-5M capacity win observed at seed 42 does **not** replicate at seed 43. Pooled across both seeds, the effect is within cross-seed noise. Keep `--capacity 5000000` (the default since v11).

## Capacity sweep

**Setup:** identical configuration to v33-B / v35-A except `--capacity`, 20M iterations, seed 43, 5000-deal evals.

| run | seed | capacity | best (mbb) | @iter |
|---|---|---|---|---|
| v33-B | 42 | 5M | 2728.6 | 10M |
| v35-A | 42 | 60M | 2650.1 | 16M |
| v36 cap5M | 43 | 5M | **2638.5** | 16M |
| v36 cap60M | 43 | 60M | 2777.7 *(provisional, 7/10 readings)* | 6M |

Per-seed delta (60M − 5M):

| seed | delta |
|---|---|
| 42 | −78.5 mbb |
| 43 | +139.2 mbb *(provisional)* |

**Pooled across 2 seeds:** 60M is +30.4 mbb *worse* (provisional). Cross-seed SD is ~110 mbb. Effect is deep within noise.

**Conclusion:** The seed-42 capacity win was seed noise. Revert any plan to change the default capacity.

## Cross-seed variance observation

Handoff §2 documented SD = 78 mbb across 5 seeds at 5M iterations. This session, at 20M iterations, cross-seed spread is 90–130 mbb on matched configs.

The SD grows with iteration count — more trajectory divergence.

## Sync sweep

Phase 2 has not started (waiting on cap60M-seed43 to complete). Will be documented separately.

## Recommendations

1. **Keep `--capacity 5000000` as the default.** Do not change.
2. **Update the handoff's cross-seed SD guidance:** ~78 mbb at 5M, ~110 mbb at 20M.
3. **Best 20M iteration reading this session: 2638.5 mbb** (v36 cap5M seed 43). Below both seed-42 anchors.

## Artifacts

- `outputs/v36cap/cap5M-seed43/` (complete)
- `outputs/v36cap/cap60M-seed43/` (in progress)
