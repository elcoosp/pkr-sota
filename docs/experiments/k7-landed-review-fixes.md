# K=7 action abstraction - landed + review fixes (2026-10-10)

## The K=7 change (checkpoint-invalidating, by design)

- NUM_ACTION_BUCKETS 6 -> 7; BET_SIZINGS [0.5,1,2] -> [0.5,1,2,3].
- Buckets: 0 fold, 1 check/call, 2 =<0.75pot, 3 <1.5pot, 4 <2.5pot,
  5 >=2.5pot (3x), 6 JAM.
- Fingerprint action_legal_v 1 -> 2, so pre-K7 checkpoints refuse to load.
- V5 base-7 preflop packing fixed (digits 0..6, no +1 which would be 7).

## A code review found the K=7 change left downstream consumers behind

All three findings were CONFIRMED by grep against the code:

1a translate_bet (runtime/translate_live.rs) had NO 3.0x anchor and
   returned bucket 5 for a jam. FIXED: ladder derived from BET_SIZINGS,
   jam -> JAM_BUCKET. Round-trip test added (every legal bet translates
   to the bucket action_bucket assigns, 500 draws each).
1b the decode loops in tournament.rs, lib.rs and pkr-bot-pipe.rs were
   hard-coded to 6 buckets -> the JAM bucket was dropped and its mass
   renormalised. FIXED to NUM_ACTION_BUCKETS. Jam regression test added.
1c traversal.rs pruning exempted `a != 5` (the K=6 jam index) -> under
   K=7 it exempted the 3x bet and PRUNED the jam. FIXED to JAM_BUCKET.
1d CI gate ci/scripts/check-no-hardcoded-k.sh; also fixed additional
   stale literals the gate found (vector.rs, subgame/lib.rs, spot.rs).

## CORRECTION to the review: past results are NOT affected by 1b

The review hypothesised that the V4/V5 H2H, the search +1.18 chips/deal
and the arena number were measured with jam-incapable agents. That is
WRONG. Verified with git: before this K=7 change, jam was bucket 5 and
the decode loop was `0..6` (indices 0..5) -- which INCLUDES bucket 5.
The harness was correct for K=6.

1b is a bug I introduced THIS SESSION when I flipped K to 7 and did not
update the consumers. It never ran in any measurement. So the earlier
V4/V5 and search results are still valid for the K=6 abstraction. They
are, however, no longer representative of the (now K=7) code -- a
re-baseline on K=7 is warranted, but that is a new question, not an
invalidation of the old numbers.

## Validated

- Full workspace: 0 compile errors; all crate tests pass
  (core 82, cfr 67, subgame 24, runtime 25, fuzz 16, exploit 34,
  abstraction 51, export 44).
- K=7 smoke: trainer runs, writes a K=7 checkpoint + blueprint,
  lbr_eval RELOADS it (k=200, sig=5, 1905246 keys) with no fingerprint
  mismatch. bytes_per_infoset 96 -> 112.
- CI gate clean.
