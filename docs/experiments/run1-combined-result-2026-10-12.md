# Run 1 result: combined eps=0.002 + strict min-raise at 100M (2026-10-12)

**Verdict: DO NOT SHIP.** The combined change is a mild regression, not a
win. Neither plan section 3 gate is cleared.

## The numbers

| metric | k7-baseline (shipped) | run 1 | change |
|---|---:|---:|---:|
| LBR (20k, seed 42) | -162.79 +/- 104.49 | **+98.54 +/- 114.27** | +261 (1.68 SE) |
| H2H search-vs-bp seed 42 | +0.958 (t=+5.28) | +1.105 (t=+6.22) | wash |
| H2H search-vs-bp seed 43 | +1.279 (t=+7.02) | +1.078 (t=+5.94) | wash |
| in-sample BR @ 25M | 8148 | 8180 | +32 |
| in-sample BR @ 50M | 7874 | 7955 | +81 |
| in-sample BR @ 75M | 7864 | 7933 | +69 |
| in-sample BR @ 100M | 7922 | 8062 | +140 |

The in-sample BR is monotone-worse at every eval point (never crosses
over). Combined with the LBR signal, that is two independent measures
agreeing: run 1 is slightly weaker.

## Why the comparison is valid

Before drawing any conclusion the loose k7-baseline was re-LBR'd with
the CURRENT binary (P13 touched `lbr.rs`; M-3 touched the fingerprint
logic). It reproduced byte-for-byte: `-162.79 +/- 104.49`, same as
before this session's changes. So the delta is attributable to the run
1 config, not to a code change in the LBR path.

## Structural caveat (from the design doc)

`PKR_STRICT_MIN_RAISE=1` bumps `action_legal_v` 2 -> 3, so run 1 and
the k7-baseline are DIFFERENT ABSTRACT GAMES. They cannot cross-load,
and their LBRs are not strictly comparable. The design doc said this
before the run: the +261 is suggestive, not a clean 1.68 SE.

But the in-sample BR, which IS measured on run 1's own game, also got
worse at every point. So run 1's own numbers say "weaker", not just
"differently measured".

## What this rules out

- eps=0.002 + strict, together, as a shipping change: ruled out.
- The Leduc signal for eps=0.002 (35% better on Leduc, 3M iters) does
  NOT carry to NLHE at 100M in this configuration.

## What this does NOT rule out

- eps=0.002 alone (strict might be the cause; the two changes were
  bundled).
- strict alone (eps might be the cause).
- Either change at a different iteration budget.

To distinguish "eps hurt", "strict hurt", or "both are fine and the
combination is bad luck", you would need to run each at 100M with the
other held at the k7-baseline value. That is two more ~6h runs. The
prior from run 1: both are probably a wash or mildly negative; the
upside is not compelling enough to spend 12 more GPU-hours without a
better reason than "the Leduc grid looked good".

## Recommendation

Keep `k7-baseline` as shipped. The K=7 + eps=0.01 + loose-min-raise
configuration stands. If a future session wants to revisit either
knob, do it as a clean 2-seed A/B at 100M, not another combined run.

## Artifacts

- `outputs/run1-combined/{train.ckpt, blueprint.bin, metrics.csv,
  exploitability.csv, stats.json}` (train.ckpt.prev removed to save
  disk)
- `/tmp/run1-result.txt` -- the phase output
