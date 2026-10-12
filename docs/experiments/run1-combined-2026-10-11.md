# Run 1: combined "wins" at 100M (2026-10-11)

**Status:** RUNNING. Launched Sun Oct 11 20:03, ETA ~7.5h.

## The question

Two changes never evaluated at convergence:

1. **`PKR_EXPLORE_EPSILON=0.002`** -- Leduc grid says ~35% lower
   exploitability than the default 0.01
   (`docs/experiments/leduc-cfr-grid-2026-10-10.md`). The 20M NLHE
   sweep was noisy (see `eps-sweep-2026-10-11.md`) but leaned the
   same way.
2. **`PKR_STRICT_MIN_RAISE=1`** -- the M-3 correctness fix landed this
   session. Never evaluated.

Run them TOGETHER at the baseline's exact budget (100M iters, seed 42,
cap 60M) and compare to the shipped `k7-baseline` (eps=0.01, loose).

## Config

---

**RESULT (2026-10-12):** DO NOT SHIP. See
`run1-combined-result-2026-10-12.md`. The combined change is a mild
regression (+261 mbb LBR, 1.68 SE; in-sample BR worse at every eval).
