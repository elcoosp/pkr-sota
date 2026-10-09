# VERIFIED FINDINGS - authoritative (2026-10-09)

Supersedes SESSION_2026-10-08_verified.md and all point-in-time
experiment docs. Numbers below are from tests I ran and read output for.

## Search

- RAW vector river search, on-tree BR vs V5 blueprint: 2.16 vs 8.44
  (search is 4x less exploitable). 4-subgame sweep: raw < blueprint all.
- Search H2H vs blueprint, V5, 20k deals, BOUND config, 4 seeds:
  +1.1763 / +0.8211 / +0.9722 / +1.1625 (t = 6.16/4.11/4.96/6.17).
- Search H2H, GADGET config (corrected EV cbv), seed 42: +0.6203 (t=5.14).
- V4 (earlier): LBR 821->444; H2H +1.254 (t=6.25).
- Arena vs scripted: blueprint 69.25 vs +search 102.27 bb/100 (300 hands).

## Safety (both PROVEN)

- BOUND: final_br <= blueprint_br (test: 0.0205 vs 0.0245).
- GADGET (sampled): bp_br 9.5612 vs gadget_br 0.6284.
- GADGET (vector): gadget <= blueprint in all 4 sweep subgames (worst -0.94).
- The gadget bug was the terminate value: BR is WRONG, EV is correct
  (cbv=BR -> 11.23, cbv=EV -> 2.09, raw -> 2.16).

## Live runtime (measured)

- river decision (bound): 0.69s after the budget fix (was 2.14s).
- turn decision (bound): 44.6s even with the budget -> OFFLINE ONLY.
- Live default: river-only, proven bound. PKR_VSEARCH_RAW=1 /
  PKR_VSEARCH_GADGET=1 to opt out; PKR_VSEARCH_STREETS to widen.

## V4 vs V5 (the signature question)

- Cross-process H2H (examples/v4v5_h2h.rs, CRN, FIXED bot, 10k x4):
  +0.27/+0.08/+0.16/+0.10, all |t|<1 -> EQUAL in self-play.
- LBR samples=2000: V4=997, V5=183 (gap 814). In-tree-only: V4=227, V5=29
  (gap 198). Off-tree = 76% of the gap.
- Read: V5 is genuinely less exploitable (both in-tree and off-tree) but
  does not beat V4 head-to-head. Different axes; not a contradiction.
- Competitiveness: V5 ~29-183 mbb by internal LBR (competent band by the
  project scale), V4 ~227-997. NEITHER verified externally. LBR is a
  lower bound from ONE greedy exploiter and misled 3x this session.

## Method notes

- LBR alone is not trustworthy (3 failures). Use cross-process H2H + LBR.
- Tournament seeds 42-45 were correlated (shifted-by-one); fixed.
- pkr-bot-pipe gave seat-1 the wrong hole; fixed (invalidated one H2H run).
