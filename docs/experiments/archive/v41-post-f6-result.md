> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

# v41 — inconclusive, binary was pre-audit

**Date:** 2026-09-30
**Status:** wasted run. Relaunched as v42.

## What happened

v41 was launched at 14:48 on a binary built at **11:00**, which was
from before commits `e73e39c` (F6) and `72ad117` (F2). The current
binary was built at 16:22.

Consequences:

1. **The checkpoint has `action_legal_v=0` (fingerprint byte 35)** — the F6 fingerprint
   guard rejects it. `pkr-arena` and every test that loads a v41
   artifact fails with `action_legal_v mismatch: stored=0 current=1`.
2. **The training config was the historical default, not F2's** —
   `stats.json` records `PKR_EXPLORE_EPSILON=0.05` and
   `PKR_MOMENTUM=on`. The F2 fix's defaults (eps=0.01, momentum=off)
   were not applied because the binary predated it.
3. **The estimator was post-F1** — 5000-deal SEs were honest, which
   is why the SKIP-PROMOTE lines fire with sigma_needed ~350 mbb.

## Recorded readings (for the record, not for comparison)

| iter | expl_mbb | SE |
|---|---|---|
| 3.0M | 4152.3 | 185.1 |
| 6.0M | 3947.4 | 184.8 |
| 9.0M | 3721.4 | 176.7 |
| 12.0M | 3599.5 | 184.5 |
| 15.0M | 3349.6 | 175.0 |
| 18.0M | 3339.4 | 176.6 |
| 21.0M | 3375.1 | 174.6 |
| 24.0M | 2988.7 | 176.0 |
| 27.0M | **2526.8** | 163.6 |
| 30.0M | 3274.3 | 175.0 |

Best: 2526.8 @ 27M. This is on the OLD legal-action tree and the OLD
config. Not comparable to any post-audit result.

## Lesson

The launcher must verify the binary is newer than the most recent
commit affecting `crates/pkr-cfr/`, `crates/pkr-core/`, or
`binaries/pkr-trainer/`. Add that check to the next launcher.
