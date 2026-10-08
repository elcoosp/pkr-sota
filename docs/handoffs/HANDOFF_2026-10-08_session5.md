# HANDOFF - 2026-10-08 session 5 (search bound + gadget + V4/V5)

Continues HANDOFF_2026-10-07_session4.md.

## V4 vs V5: reconciled (supersedes the earlier "artifact" framing)

At equal (stronger) exploiter strength, samples=2000:
  V4 LBR = 996.79   V5 LBR = 182.84   gap ~814 mbb
(At samples=400: V4=821, V5=-2, gap 823.)

The V4-vs-V5 LBR gap PERSISTS at a stronger exploiter, so it is NOT a
weak-exploiter artifact. BUT cross-process H2H (10k x4 seeds, CRN) says
V4 == V5 (all flat, |t|<1).

HYPOTHESIS (unverified, test running): LBR exploits V4 via OFF-TREE bet
sizes that neither bot plays in self-play; H2H only uses in-tree sizes,
so the off-tree exploit is never exercised. If true: V5 is ~800 mbb more
robust vs OFF-TREE aggression (real), while in-abstraction they are equal.

Test in flight: lbr_eval --no-offtree (in-tree-only LBR) for V4 and V5.
  - If the gap vanishes with --no-offtree -> off-tree hypothesis CONFIRMED.
  - If it persists -> the gap is in-abstraction, and H2H vs LBR disagree
    for a different (unexplained) reason.

See docs/experiments/v4-vs-v5-h2h.md and v4-vs-v5-lbr-reconciled.md.

## What holds regardless

- Vector search HELPS: H2H on V5, 4 seeds +1.18/+0.82/+0.97/+1.16 (t>4);
  gadget-bounded seed 42 +1.108 (t=5.68).
- CFR-D gadget PROVEN on-tree (blueprint_br 8.44 vs gadget_br 2.18;
  4-case sweep all below). Requires infoset-signature threading
  (byte-verified, 1356 nodes, 0 mismatches).

## Do NOT claim a competitive bot

V4 LBR floor ~800-1000 mbb. No instrument shows world-class strength.

## Running at handoff

- /tmp/gadh2h.sh: gadget search H2H seed 43.
- /tmp/nooff.sh: LBR --no-offtree V4 and V5 (tests the off-tree hypothesis).
