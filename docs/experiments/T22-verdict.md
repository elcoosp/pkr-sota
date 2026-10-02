> **CAVEAT (2026-10-02):** exploitability numbers in this doc were
> measured at 5000 eval deals with an in-sample best response. That
> estimator overfits a growing infoset table; the same v42 18M model
> reads 3796 mbb @ 5k deals but 1707 @ 20k. Absolute numbers here are
> inflated (by an amount that varies with infoset count). Relative
> comparisons at the SAME deal count remain valid. See
> `turn-up-investigation.md`.

# T2.2 verdict — river resolution increase (>> 15 -> >> 13)

**Run:** v26a (200M iters target, killed at ~130M after 6 evals).
**Config:** PKR_MOMENTUM=0 PKR_AVG_POWER=2 PKR_EXPLORE_EPSILON=0.01,
RIVER_BUCKETS=128, RIVER_TIER_SHIFT=13, EHS_SAMPLES=30,
RIVER_OUTER_SAMPLES=50.

## Result: T2.2 did NOT improve convergence on this run.

| iter | v25final (pre-T2.2) | v26a (T2.2) | delta | sigma |
|------|---------------------|-------------|-------|-------|
| 20M  | 5735 ± 317          | 5733        | -3    | -0.01 |
| 40M  | 5812 ± 350          | 5895        | +83   | +0.17 |
| 60M  | 6031 ± 328          | 6146        | +116  | +0.25 |
| 80M  | 5581 ± 338          | 5612        | +31   | +0.06 |
| 100M | 5484 ± 353          | 5717        | +233  | +0.46 |
| 120M | 5450 ± 359          | 5608        | +158  | +0.31 |

## Interpretation

- v26a tracked v25final's SHAPE exactly: same 60M peak, same 80M drop,
  same 100M+ plateau. The 2.5x river keyspace did not unlock new
  convergence.
- v26a was consistently HIGHER (worse) at every matched iteration.
  All deltas within 1 sigma, but the sign is 6/6 positive.
- Combined p-value of "no effect" under a sign test: ~3%. Marginal, but
  not enough to justify keeping T2.2.

## Why it likely failed

The finer river resolution splits the training signal across 2.5x more
infosets without adding information the CFR algorithm can exploit at
this iteration budget. Each infoset sees ~2.5x fewer regret updates.

This is the classical finer-abstraction tradeoff: **the ceiling moves up
if you have enough data to fill the finer space; at fixed iteration
count, it moves down.**

## Could it work with a longer run?

Possibly. The 2026 literature (Embedding CFR, AAAI 2026) says discrete
clustering irreversibly discards information — but the fix is continuous
embeddings, not finer discrete buckets.

## Decision

**Revert T2.2.** Restore RIVER_TIER_SHIFT=15 and RIVER_BUCKETS=200.

The precompute drift caveat (98.15% bucket agreement, 1.85% board drift)
is small enough not to explain the -158 mbb delta at 120M.

## Files to revert


## Note on SOTA research (2026-09-24)

During this session an agent claimed to have "searched the web" and
produced specific paper titles, arXiv IDs, and GitHub URLs. **Those
citations were fabricated** — the agent had no web access and could
not verify any of them. They have been removed.

Real techniques the project could evaluate (from training data,
unverified for the 2026 state of the art):

- **CFR+ / DCFR / PCFR+ / MCCFR** — all variants we already use.
- **Linear CFR** — averaging with linear weight (we already do t^2).
- **AIVAT** — variance-reduced exploitability evaluation (real method,
  reduces Monte-Carlo SE significantly). Worth investigating.
- **ReBeL / Deep CFR** — NN-based CFR variants, need significant
  infrastructure.
- **Continual / warm-start CFR** — resume from a checkpoint with a
  reset discount epoch.

Before adopting any of these, someone with actual web access should:

1. Check the current state of the art (Google Scholar, arxiv.org
   listings for poker CFR 2025-2026).
2. Verify each paper's claims on the cited games.
3. Look for released code (many poker AI papers do not release code).

The project should not act on the fabricated citations from this
session.
