# Session findings — 2026-10-03..05 (competitiveness)

Consolidates the session's measured results. All LBR numbers are
real-game lower bounds (lbr.rs, 20k hands, seed 42, off-tree sizes
[0.33,0.75,1.5,3.0]) unless noted. In-sample (abstract-BR) numbers are
NOT used for any conclusion — they are inflated and rank tables
backwards (report R1, demonstrated).

## 1. The signature-dimension ladder (proven lever)

| arm | added dim | infosets | LBR mbb/hand |
|---|---|---|---|
| V1 | — | 1.47M | 1115.6 |
| V3 | faced bet size | 4.07M | 874.5 |
| V4 | + pot/SPR class | 6.79M | 770.7 |

**V1 -> V4 = -344.9 mbb, -30.9%, 2.45 sigma.** Each correct coarse
missing dimension lowers real-game exploitability. The in-sample metric
ranked them BACKWARDS (2647 < 4940 < 6215). Gated: PKR_SIG_V3/V4/V5.

Two failed attempts recorded (do not repeat): full action sequence =
31x table blow-up; 5-bit pot class + prev-aggr = 41x. The fix is
MINIMAL fields.

V5 (prev-street aggressor, 2.44x growth) built, not yet run.

## 2. Search — the largest measured lever

LBR with search (CFR-D safe gadget, river+turn, 50 iters, 12 hands/range)
vs without, PAIRED on the same deals:

| seed | no-search | +search | paired delta | t |
|---|---|---|---|---|
| 42 | 716.3 | 89.0 | -627.3 | -5.44 |
| 99 | 762.9 | 138.5 | -624.4 | -5.41 |

**Replicated across two deal sets, t ~ -5.4.** The paired design
(lbr_per_hand_mbb) gave SE ~115 vs unpaired ~440 — 4x tighter.

CAVEAT: search solves assuming the opponent follows the blueprint;
LBR does not. The magnitude is likely overstated by LBR's weakness
(a greedy exploiter, not a full BR). Direction is solid; "6-8x" is an
upper bound. A head-to-head confirmation is the next step.

Cost: search is ~6x slower per hand. Mechanism verified sound
(decide -> safe_resolve_gadget, theorem-backed, non-degenerate root).

## 3. Abstraction depth — k=400 (u16 migration)

- Tables were u8 with a hard k<=255 assert. Migrated to env-gated u16
  (PKR_BUCKET_U16): reader, flop/turn/preflop/river writers, fingerprint
  bucket_width guard. VERIFIED byte-faithful: k=200 u16 at
  EHS_SAMPLES=100 decodes to EXACTLY v42's u8 table (0 mismatches /
  25,989,600).
- Real build times: flop 2 min, turn 34 min. The "2.5 day" doc figure
  is for `turn-potential` (F4), NOT plain `turn` — the docs conflated
  two commands.
- k=400 LBR verdict pending (k400v4b, ~8h).
- GOTCHA: a stale binary predating the u16 field wrote bucket_width=0
  and trained on garbage. Always check binary mtime vs sources.

## 4. The metric inversion (report R1, the root cause)

The in-sample abstract-BR is fit+scored on the same 5k deals; its bias
grows with infoset count. Same checkpoint reads 3796 @5k -> 1707 @20k ->
1222 @40k. Consequences:
- Every "negative" result (F4 potential, k=250, T2.2, SIG_V2) was
  decided on this metric -> VOID. Re-test with LBR.
- plateau-stop and promote-gate fire on it -> bigger tables penalised.

## 5. Method (what to trust)

- PAIRED LBR on the same seed (lbr_per_hand_mbb + --dump) for any
  config comparison. Unpaired is ~4x noisier and inconclusive at
  affordable hand counts.
- One seed is suggestive; the ladder and search each replicated (search
  2 seeds). Treat single-seed <1.5 sigma as noise.
- Never quote in-sample absolute numbers.

## 6. Also landed (machinery)

- CFR-D root gadget (safe_resolve_gadget) + safety test.
- Leduc known-Nash CI gate (converges 21.02 mchips, 4s).
- Live translation wiring (PKR_TRANSLATE, §B), exact preflop (§C),
  iteration-overflow guard (§E).
- u16 migration (see 3).
