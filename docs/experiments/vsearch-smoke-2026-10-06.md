# Vector-search smoke tests (2026-10-06, pre-measurement)

While the V4/V5 A/B ran, ran the exact launcher command shapes at tiny
counts to prove the queued measurement will not abort or hang.

## What was verified

1. **The search fires.** With `PKR_SEARCH_DEBUG=1` and an atomic
   counter added to `vsearch::solve_root`:
   - LBR with `--search-vector --search-streets r` on 50 hands:
     `solve_root invoked 7 times, street gate refused 0 times`.
   - LBR on 200 hands: 19–20 invocations.
   - Tournament `--search-a --search-vector`: nonzero
     A-vs-B diff at 20 deals (i.e. search actually changed decisions).

2. **Search converges quickly.** 200-hand LBR, seed 42, 50 samples:
   | iters | mbb/hand |
   |---|---|
   | 3 | -1091.25 |
   | 30 | -1025.00 |
   | 100 | -1025.00 |
   The 30- and 100-iter results are byte-identical: the vector river
   solve converges well before 100 iters. The queued measurement uses
   100 — safe, slightly wasteful.

3. **Baseline vs search (200 hands, 400 samples, seed 42):**
   | config | mbb/hand | SE |
   |---|---|---|
   | blueprint-only | -713.75 | 876.5 |
   | search, 100 iters | -1450.00 | 955.8 |
   The difference is -736 mbb, but the SE of the difference is ~1240
   (|t| ≈ 0.6) — **not significant at 200 hands**. The queued 20k-hand
   measurement will have SE ≈ 88 mbb per arm, resolving the difference
   to |t| ≈ 6 *if* it is real.

4. **Sign convention reminder.** `mbb_per_hand` = chips the greedy LBR
   exploiter wins per hand (milli-big-blinds). Lower = the bot is less
   exploitable. Both arms are negative here, i.e. the LBR exploiter
   loses chips — a strong bot. The question is whether the search moves
   the number in the *right* direction *and* whether the H2H agrees.

## The trap this measurement is designed to avoid

Handoff §2: for the *sampled* subgame solver, **LBR said it helped
(-627 mbb, t ≈ -5.4)** but **H2H said it lost -6.06 chips/deal
(t = -6.26)**. LBR is a greedy myopic exploiter — it cannot punish an
unusual-but-losing policy, so it scored the bad searched policy as
*less exploitable*.

The vsearch smoke gives the *same pattern* the sampled search did:
the search scores better on LBR. Whether it is genuinely better will
be decided by the H2H — which the queued launcher runs.

**The decision rule (unchanged):** ship search only if BOTH
- LBR `search < blueprint − 2·SE` (says less exploitable), AND
- H2H `mean_diff(A=search − B=blueprint) > +2·SE` (says it wins).

If LBR improves but H2H is flat/negative, the vector search inherits
the sampled solver's fate: parked.

## Not verified in this smoke

- The queued **20k-hand** measurement (running after the A/B).
- Turn search (`--search-streets tr`). The queued run is river-only,
  as the plan (§4.5) allows. Turn can be a follow-up.
- The CFR-D gadget. `VSearchHandle` returns the raw solve — no safety
  bound. Even a positive H2H result would need the gadget before
  shipping to a real opponent.
