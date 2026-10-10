# K=6 vs K=7: not comparable (2026-10-10)

K=6 and K=7 are DIFFERENT ABSTRACT GAMES. No H2H between them is
available in this repo, and the reason is structural, not a missing flag:

1. CHECKPOINT STRIDE: `RM_STRIDE = K * RM_FIELDS`. A K=7 binary cannot
   load a K=6 train.ckpt (stride mismatch); a K=6 binary cannot load a
   K=7 one. So the two cannot meet in `tournament` / `lbr_eval` /
   `v4v5_h2h` (all load checkpoints).
2. INFOSET KEYS: `action_bucket` changed, so the SAME GameState hashes
   to a different key under K=6 vs K=7. A K=6 blueprint is all-misses to
   a K=7 binary. Blueprint-level cross-play (which reads `max_actions_k`
   from the header) still fails because the KEYS differ, not just K.

## What this means for the re-baseline

The honest measurement of K=7 is its OWN LBR + H2H against a K=7
opponent (search on/off, or a within-K=7 config A/B). Comparing its
LBR to the historical K=6 numbers (overnight-v4 = 821 etc.) is
suggestive only: different game, different key, different abstraction.

## What is runnable and valid

- K=7 own LBR (queued with the baseline train).
- Within-K=7 A/B: two K=7 checkpoints differing in one knob (e.g. search
  on vs off, or PKR_LINEAR_CFR), H2H via tournament (same K).
- K=7 search-vs-blueprint H2H (same K) via tournament --search-a.

## Not runnable

- K=6 vs K=7 H2H (stride + key). Would need a cross-key runtime bridge.
