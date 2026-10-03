# V3 size-aware signature is non-viable as implemented (2026-10-03)

**Status:** negative result. V3 makes the abstract game ~31× larger and
does not converge in budget. Do not re-enable without a redesign.

## What happened

A 100M-iteration A/B, V1 (`PKR_SIG_V3=0`) vs V3 (`PKR_SIG_V3=1`), seed
42, eval every 10M at 10k fixed-seed deals.

| iter | V1 infosets | V3 infosets | V1 expl | V3 expl |
|---|---|---|---|---|
| 10M | 1,159,399 | 35,978,364 | 2183 | 10657 |

V3 is **31× larger** at the same iteration. Causes:

1. **The V3 encoding is a near-unique history fingerprint.** It packs the
   *full within-street action-bucket sequence* (3 bits × 7 actions = 21
   bits) plus pot class and raises. Every distinct action sequence is its
   own infoset — it does not just add "which bet size am I facing".
2. Consequently each infoset is visited ~3 times by 100M iters (vs ~86
   for V1), so V3 is drastically undertrained.
3. 60M capacity is hit around 20-30M iters → panic, or disk exhaustion
   first (3.8 GB/checkpoint vs 130 MB). The run was killed at ~11M.

## What this does and does not show

- It does **not** show that fixing imperfect recall is worthless. The
  report's R2 (the key cannot see the bet size faced) stands.
- It shows **this encoding** over-corrects into a different, intractable
  game. The in-sample metric also penalises the larger table (R1), so
  the 10657 is not a real-game comparison — but the size blow-up alone
  makes it unviable.

## What a correct fix looks like

Add **only the missing information**, not the full sequence:

- the **faced bet-size bucket** (which of {0.5, 1, 2, jam} the opponent
  bet) — 2 bits;
- the **pot/SPR class** — a few bits;
- keep `(actions_this_street, total_raises, last_was_bet)` as V1 has.

That is ~5-6 bits, not 21. It should grow the table by ~2-4×, not 31×.

## V1 baseline (kept)

V1 finished 100M: best **2183 mbb @ 10M**, rising to 2647 @ 100M (the
in-sample artifact — still not converged at 10k deals). First/last
checkpoints saved for LBR ranking.
