# HANDOFF — signature ladder, search, k=400 (2026-10-06)

**Continues:** `docs/handoffs/HANDOFF_2026-10-02_final.md`
**HEAD:** see `git log -1`  **Tree:** see `git status`

---

## Section 1 — Running at handoff

- **Fast H2H search vs blueprint** (`/tmp/h2h2-result.txt`): 3000 hands,
  `PKR_SUBGAME_MAX_RIVERS=6`, V4 checkpoint, A=search, B=blueprint. This
  is the decisive INDEPENDENT check on search (LBR may mislead — see §3).
  ~30 min; ~11x faster than the uncapped run.
- Nothing else running. Disk tight (~12-20 GB free). `overnight-k400v4b`
  (the invalid stale-binary run) was deleted.

---

## Section 2 — The signature ladder (the proven lever)

LBR real-game lower bound (20k hands, seed 42, off-tree sizes
[0.33,0.75,1.5,3.0]), 100M iters, k=200, same tables:

| arm | added dimension | infosets | LBR mbb/hand |
|---|---|---|---|
| V1 | — | 1.47M | 1115.6 |
| V3 | faced bet size | 4.07M | 874.5 |
| V4 | + pot/SPR class | 6.79M | 770.7 |

**V1 -> V4 = -344.9 mbb, -30.9%, 2.45 sigma.** Each correct COARSE
missing dimension lowers real-game exploitability.

The in-sample (abstract-BR) metric ranked these BACKWARDS
(2647 < 4940 < 6215) — report finding R1 demonstrated on a real
decision. Never conclude from in-sample absolutes.

Gates: `PKR_SIG_V3`, `PKR_SIG_V4`, `PKR_SIG_V5`. V5 (prev-street
aggressor; k=8 growth 2.44x) is built but NOT run.

Two failed attempts (do not repeat): the full within-street action
sequence = 31x table blow-up; 5-bit pot class + prev-aggressor = 41x.
The successful fix is MINIMAL fields (faced-bet bucket; 2-bit pot class).

---

## Section 3 — Search: the largest lever, and the most contested

### The result (paired LBR, the honest ruler for relative comparisons)

| seed | no-search | +search (50 it, 12 hands) | paired delta | t |
|---|---|---|---|---|
| 42 | 716.3 | 89.0 | -627.3 | -5.44 |
| 99 | 762.9 | 138.5 | -624.4 | -5.41 |

Replicated across two deal sets, t ~ -5.4. The paired design
(`lbr_per_hand_mbb` + `lbr_eval --dump`) gives SE ~115 vs unpaired ~440.

### BUT — two serious caveats

1. **LBR is not monotone with strength.** A policy that becomes
   unpredictable/degenerate can score a LOWER LBR while playing WORSE
   (a coin-flip bot has near-zero LBR and loses every game). So "search
   lowers LBR" does not by itself prove "search is stronger".
2. **First head-to-head (before fixes) showed search LOSING**
   (-6.17 chips/deal). That run was corrupt (unmasked range ->
   hole/board overlap -> EHS fallback + a stale binary), so it is
   invalid — but it is why the H2H in §1 matters.

**The H2H in §1 is the tiebreaker.** A=search vs B=blueprint, same V4
checkpoint, seats swapped, same deals:
- **diff > 0, t > 2** -> search is stronger; LBR and H2H agree.
- **diff ~ 0** -> neutral; LBR overstated.
- **diff < 0, t < -2** -> search is WORSE; the LBR -627 is an artifact.

### The speed fix (makes search testable)

`PKR_SUBGAME_MAX_RIVERS=K` caps chance-node children at K evenly-spaced
cards instead of all 46. A turn subgame tree shrank 62,589 -> 8,031
nodes and per-decision time 462ms -> 40ms (~11x), at the cost of a
sampled river expectation:

| rivers | nodes | solve |
|---|---|---|
| all | 62,589 | 462 ms |
| 6 | 8,031 | 40 ms |
| 3 | 4,134 | 23 ms |

Without this, a 200-hand H2H took 20+ min; with it, 3000 hands ~30 min.

### The bug chain fixed this session (all real)

1. Unmasked uniform range in the tournament search -> hole/board-overlap
   deals -> EHS fallback -> garbage. Fixed: mask the range by board;
   solver also defensively rejects overlapping deals.
2. Stale binary predating the u16 field trained on garbage (see §4).
3. `Solver::new` / blueprint strategy / gadget value / solve all rebuilt
   per decision -> the speed problem. Partially addressed by the river
   cap; full fix is caching (see §6).

---

## Section 4 — Abstraction: k=400 NEGATIVE; the u16 migration

### k=400 result

| k | infosets | LBR mbb/hand |
|---|---|---|
| 200 (V4) | 6.79M | **770.7** |
| 400 | 8.72M | **877.7** |

k=400 is ~0.9 sigma WORSE. Keep k=200. (Caveat: the 400 centroids were
fit with only 4000 samples = 10/cluster; a proper test refits with
>=20k samples and verifies non-degenerate buckets.)

### u16 migration (so k>255 is POSSIBLE)

Tables were `Vec<u8>` with a hard `k <= 255` assert. Migrated to
env-gated u16 (`PKR_BUCKET_U16=1`): reader (`read_bucket`), flop/turn/
preflop/river writers, fingerprint `bucket_width` guard.
**Verified byte-faithful:** a k=200 u16 flop table at EHS_SAMPLES=100
decodes to EXACTLY v42's u8 table (0 mismatches / 25,989,600).

Build times: flop 2 min, turn 34 min. **The "2.5 day" doc figure is for
`turn-potential` (F4), NOT plain `turn`** — the docs conflated two
commands.

GOTCHA: a checkpoint written before the `bucket_width` field has
stored=0; the loader now treats that as "unknown, accept current".
Always check the training binary's mtime vs the u16 source commits.

---

## Section 5 — The metric (root cause of many wrong conclusions)

The in-sample abstract-BR is fit+scored on the same 5k deals; its bias
grows with infoset count. Same checkpoint: 3796 @5k -> 1707 @20k ->
1222 @40k. Consequences:
- Every "negative" result decided on it (F4 potential, k=250, T2.2,
  SIG_V2) is VOID — re-test with LBR / H2H.
- plateau-stop and promote-gate fire on it, penalising bigger tables.

**Method to trust:** paired LBR on the same seed (`lbr_per_hand_mbb` +
`--dump`) for relative comparisons; H2H for an independent check;
NEVER quote in-sample absolutes.

---

## Section 6 — Bugs / machinery landed this session

- CFR-D root gadget (`safe_resolve_gadget`) + safety test
  (bp_br 64.86 -> gadget_br 0.63 on a real fixture).
- Leduc known-Nash CI gate (converges 21.02 mchips, 4s).
- Live translation wiring (`PKR_TRANSLATE`, §B), exact preflop (§C),
  iteration-overflow guard (§E).
- u16 migration (§4).
- Search: masked range, defensive solver guard, `EvalContext.search`,
  `pkr-tournament --search-a/--search-b/--search-iters/--search-hands`,
  `PKR_SUBGAME_MAX_RIVERS`, `lbr_eval --dump`.
- V5 signature built (not run).

---

## Section 7 — What to do next (ranked)

1. **Read `/tmp/h2h2-result.txt`.** If search wins -> it is the lever;
   pursue the FULL search-speed fix (cache tree + blueprint strategy +
   gadget value per board; warm-start). Estimated 10-50x; turns search
   from "testable" into "shippable". If search loses -> the LBR -627 is
   an artifact and search is parked until it is strong enough to beat
   the blueprint.
2. **Fix the k=400 centroids** (refit >=20k samples) and re-LBR — the
   current negative may be a bad fit, not the abstraction.
3. **Run V5** (prev-street aggressor) — cheap, same proven pattern.
4. Re-test the VOID negatives (F4, k=250) with LBR/H2H.

---

## Section 8 — Environment hazards (unchanged)

1. Multiple watchexec instances re-run every `wr1.sh` write; use
   `if mkdir /tmp/X.lock` to launch once.
2. Long heredocs truncate/duplicate — write whole files with `>`,
   verify the closing sentinel.
3. `.git/index.lock` contention from watcher `git status` — commits
   land anyway; retry.
4. `PKR_CENTROID_FEATURE_V`, `PKR_SIG_V3/V4/V5`, `PKR_BUCKET_U16` must
   match the checkpoint/tables or the load aborts (by design).
5. macOS `awk` has no `match(...,array)`.

---

## Section 9 — If you only do one thing

`cat /tmp/h2h2-result.txt`. It decides whether search is the real lever
or an LBR artifact. Everything else is secondary.

---

**END OF HANDOFF**
