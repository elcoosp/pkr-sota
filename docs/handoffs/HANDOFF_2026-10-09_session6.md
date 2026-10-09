# HANDOFF - 2026-10-09 (search bound shipped; gadget; V4/V5 resolved)

Continues HANDOFF_2026-10-08_session5.md.

## 1. SHIPPED safe search = the heuristic BOUND

- Live path (RuntimeSession::advise_vsearch): solve_root_bounded, with
  the vector gadget DISABLED (it is unproven).
- Bound safety PROVEN by test bounded_root_never_worse_than_blueprint
  (final_br 0.0205 <= blueprint_br 0.0245).
- Bound H2H positive on V5 across 4 independent seeds (CRN, fixed seed
  spacing): +1.1763 / +0.8211 / +0.9722 / +1.1625 (all t>4).
- Evidence: docs/experiments/shipped-safe-search-config.md,
  search-h2h-replication.md.

## 2. Vector gadget: NOT proven (retracted)

- Earlier "PROVEN 8.44 vs 2.18" used a BUGGY chooser (1-payee).
- After 3 fixes (chooser=payee; cbv on the vector tree; follow-only BR) it
  is STILL worse than blueprint on-tree (11.2-11.4 vs 8.44). Not a
  convergence issue (400 iters also fails).
- The SAMPLED gadget (pkr_subgame::safe_resolve_gadget) IS proven safe
  (bp_br 9.5612 vs gadget_br 0.6284) and is the one to use for a real
  opponent (via SubgameHandle).
- Evidence: docs/experiments/vector-gadget-safety-status.md.

## 3. V4 vs V5 - resolved

- Cross-process H2H (examples/v4v5_h2h.rs, FIXED bot, CRN, 10k x 4 seeds):
  +0.2685 / +0.0763 / +0.1604 / +0.1016 - all |t|<1. V4 marginally ahead,
  effectively EQUAL.
- (An earlier V4==V5 run used a BUGGY bot: pkr-bot-pipe gave seat-1 the
  wrong hole. Fixed in bd66376; see v4v5-h2h-invalid-buggy-bot.md.)
- LBR at samples=2000: V4=996.79, V5=182.84 (gap 814). With --no-offtree:
  V4=226.92, V5=28.71 (gap 198). Off-tree = 76% of the gap.
- Read: V5 is genuinely less LBR-exploitable (both in-tree and off-tree)
  but does NOT beat V4 in self-play. LBR-exploitability and self-play
  strength are different axes.
- Evidence: docs/experiments/v4-vs-v5-offtree-resolution.md.

## 4. Vector search helps (independent of the gadget question)

- V4: LBR 821->444; H2H +1.254 (t=6.25).
- V5: H2H positive 4 seeds (above); arena +33 bb/100 (prelim).

## 5. Still true

- NO competitive-bot claim. V4 in-tree LBR floor ~227 mbb.
- V4-vs-V5 signature choice: keep V4 default (equal strength, fewer parts);
  V5 is the more off-tree-robust option if facing humans/bots.

## 6. Tooling landed this session

- SearchHandle trait; --search-vector/-bound/-gadget on lbr_eval/tournament.
- arena search wired (EvalContext.search was dead before).
- examples/v4v5_h2h.rs cross-process H2H, CRN (A-vs-A == 0.0000).
- pkr-bot-pipe: real actions, board advance, correct acting-seat hole.
- tournament: decorrelated seed spacing (+ tests).
- lbr_eval --no-offtree; solver boundary --sig as Option<u8>.

## 7. Open / next

- Why the vector gadget fails structurally (not chased).
- Optional: real vs Slumbot once network + the sampled gadget path are wired.
- Optional: 200bb build (Slumbot stack).
