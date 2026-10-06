# V4 vs V5 signature A/B — RESULT (2026-10-06)

Ran to completion as `/tmp/overnight-ab-result.txt`:

- **arm v4** (`PKR_SIG_V4=1`): 100M iters, 6,789,135 infosets
- **arm v5** (`PKR_SIG_V5=1`): 100M iters, 13,647,177 infosets
- Same seed (42), same preflop-exact tables, one variable (signature).

## LBR (20,000 hands, seed 42)

| arm | mbb/hand | SE |
|---|---|---|
| **V4** | 821.08 | 84.24 |
| **V5** | **-2.11** | 98.75 |

Δ = **-823 mbb**, combined SE ≈ 130, **|t| ≈ 6.3**.

## Independent instruments

**Arena** (blueprint vs scripted bots, 3,000 hands, seed 42):

| arm | bot bb/100 | vs station | vs nit | vs aggro |
|---|---|---|---|---|
| V4 | 192.13 | +212.94 | +68.27 | +295.17 |
| V5 | **202.90** | +243.02 | +63.56 | +302.11 |

V5 wins arena by ~10.8 bb/100 (both blueprints land 95.8% hits).

**In-sample BR** (trainer's own eval, 10k deals):

| arm | expl_mbb @ 100M |
|---|---|
| V4 | 6147.29 |
| V5 | 6117.45 |

V5 slightly better; the two are close because the in-sample BR is
inflated and not a strength measure — that's why LBR + arena are the
headline.

## Verdict

By the handoff decision rule (V5 < V4 - 2·SE → keep V5): **keep V5**.
`PKR_SIG_V5=1` becomes the default signature going forward.

All three instruments agree in direction; LBR and arena are
independent measurement paths.

## Caveat (the handoff's warning)

Handoff §2: LBR alone lied once — it scored the *sampled* subgame
search as less exploitable (-627 mbb) while head-to-head that search
lost 6 chips/deal. The lesson: LBR can be fooled by an unusual policy.

**Is V5 "unusual"?** No evidence of it: V5 is a *finer* abstraction of
the same game (a preflop sequence + SPR class), not a different search
policy. Arena — which plays the blueprint's own action distribution
against fixed opponents — agrees. And V5's in-sample BR is also
slightly better. The three-way agreement is what makes this believable.

**V4-vs-V5 H2H is not possible in-process**: the tournament loads both
checkpoints under one fingerprint, and the sig_version differs. A
direct head-to-head would need a blueprint export with the infoset
hash recomputed under one signature — nontrivial, deferred.

## What this means for the search measurement

The queued vsearch measurement uses `outputs/ab-v4/train.ckpt` (the
A/B's V4 arm) as its base, comparing search-on vs search-off on that
fixed checkpoint. That isolates the *search* effect, independent of the
V4/V5 question. It stays valid: it answers "does the vector search
help", which is orthogonal to which signature is better.

If the search helps on V4, the follow-up would be a search-on-V5 run —
a separate measurement.
