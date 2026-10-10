# HANDOFF - 2026-10-10 (K=7 refactor landed + K=7 re-baseline running)

**Continues:** `HANDOFF_2026-10-09_session6.md`
**HEAD at handoff:** `9c6e86c`
**Plan:** `PKR_SOTA_PLAN.md` (in the chat / earlier handoffs; not on disk)

---

## Section 1 - Background jobs RUNNING at handoff

Two jobs are live. Do NOT rebuild `target/release/*` while they run (the
final LBR step loads a checkpoint and a rebuilt binary could reject it).

| Job | PID (approx) | Script | Log | Result file |
|---|---|---|---|---|
| K=7 baseline train | 59819 | `/tmp/k7base.sh` | `/tmp/k7base.log` + `outputs/k7-baseline/metrics.csv` | `/tmp/k7base-result.txt` |
| K=7 search H2H (waits) | 9576 | `/tmp/k7h2h.sh` | - | `/tmp/k7h2h-result.txt` |

### What each does
- **k7base**: trains a K=7 checkpoint, 100M iters, seed 42, V5 sig,
  cap 60M, 4 threads -> `outputs/k7-baseline/{train.ckpt,blueprint.bin,...}`.
  Then runs `lbr_eval` (20k) on it. Prints `K7BASE DONE` when finished.
- **k7h2h**: polls `/tmp/k7base-result.txt` for `K7BASE DONE`, then runs
  `tournament --search-a --search-vector --search-streets r --search-bound`
  (K=7 search vs K=7 blueprint, 20k x 2 seeds). Prints `K7H2H DONE`.

### How to read the results
```
grep -E "K7BASE DONE|mbb_per_hand|wall=" /tmp/k7base-result.txt
grep -E "K7H2H DONE|diff \(A - B\)|t:" /tmp/k7h2h-result.txt
tail -2 outputs/k7-baseline/metrics.csv
```
Expected: k7base ~several hours (100M iters at ~5-9k it/s under load);
k7h2h ~1.5h after. Both should be done within ~8h of 12:10.

---

## Section 2 - What landed this session (all committed, all tests green)

**The checkpoint-invalidating K=6 -> K=7 action-abstraction refactor
(Task 2.4):**
- `NUM_ACTION_BUCKETS` 6 -> 7; `BET_SIZINGS` `[0.5,1,2]` -> `[0.5,1,2,3]`.
- Buckets: 0 fold, 1 check/call, 2 <0.75pot, 3 <1.5pot, 4 <2.5pot,
  5 >=2.5pot (3x), 6 JAM.
- `action_legal_v` 1 -> 2 (pre-K7 checkpoints REFUSE to load, by design).
- V5 base-7 preflop packing fixed (digits 0..6, no +1 overflow).
- New consts `FIRST_BET_BUCKET`, `JAM_BUCKET` in `pkr-core/src/abstraction.rs`.

**A code review (pasted by the user) found 3 real downstream bugs from the
K flip - all CONFIRMED and FIXED:**
- **1a** `translate_bet` (runtime) had no 3.0x anchor + returned bucket 5
  for a jam -> now derives the ladder from `BET_SIZINGS`, jam ->
  `JAM_BUCKET`. Round-trip test added.
- **1b** decode loops in `tournament.rs`, `lib.rs`, `pkr-bot-pipe.rs` were
  hard-coded to 6 buckets -> the jam was dropped. Now use
  `NUM_ACTION_BUCKETS`; a jam regression test added.
- **1c** `traversal.rs` pruning exempted `a != 5` (old jam index) -> now
  `JAM_BUCKET`.
- **1d** CI gate `ci/scripts/check-no-hardcoded-k.sh`; it found and fixed
  more stale literals (`vector.rs`, `subgame/lib.rs`, `spot.rs`).

**CORRECTION to the review:** its claim that PAST results (V4/V5 H2H,
search +1.18, arena) were measured with jam-incapable agents is WRONG.
Pre-K7, jam was bucket 5 and the loop was `0..6` which includes it. The
1b bug was introduced by MY K flip THIS session and never ran in any
measurement. Old numbers stand (for K=6). See
`docs/experiments/k7-landed-review-fixes.md`.

**Also landed (earlier this session):**
- 4.6 Show-cache (`Arc<ShowTable>` per board) - numerically neutral.
- 3.4 profile (traverse 75% / flush 20% / merge <1%) -> no perf work.
- 3.1 pruning wired observable (pruned_actions -> metrics.csv -> CI parser).
- 2.3 histogram features (`histogram.rs`: river_equity, turn_hist,
  flop_hist, kmeans_cdf, nearest_centroid_cdf, cdf, emd_l1) + 11 tests.
- 2.3(a) suit-iso measured 1.21x -> declined.

**Validated:** K=7 smoke trained, exported, reloaded (bytes_per_infoset
96 -> 112). Full workspace: 405 tests pass, 0 compile errors.

---

## Section 3 - THE key limitation for the next session

**K=6 and K=7 are DIFFERENT ABSTRACT GAMES. No K6-vs-K7 H2H exists.**
- Checkpoint stride `RM_STRIDE = K*RM_FIELDS` differs -> a K=7 binary
  cannot load a K=6 ckpt and vice versa.
- `action_bucket` changed -> the same GameState hashes to a different
  key under K=6 vs K=7 -> a K=6 blueprint is all-misses to a K=7 binary.

So: measure K=7 on its OWN terms (its own LBR, K=7 search-vs-blueprint
H2H). Its LBR vs the historical K=6 numbers (overnight-v4=821) is
suggestive only. See `docs/experiments/k6-vs-k7-incomparable.md`.

---

## Section 4 - Plan status (what's done / left)

DONE: 0.1-0.3, 0.5, 1.1-1.3, 2.1, 2.2, 2.3(features; 2.3a declined),
2.4 (this session), 3.1, 3.4, 4.1, 4.2, 4.4, 4.6.
PARTIAL: 3.2 (sweep ran: ips=256 LBR 138 vs ips=2048 590, |t|~2 - leans
256 but one seed), 3.3 (long run not completed), 4.3 (moot for river-only
shipping), 4.5 (turn unaffordable live - 45s/decision).
NOT DONE: 2.3(d/e) bucket scaling, 5 (depth-limited flop), 6 (200bb +
Slumbot - needs network + sample_api.py).
STOP LIST (0.5): GPU, public_br, PCFR+ momentum, plateau-stop, more CI.

Files: `docs/roadmap/PLAN_STATUS_2026-10-09.md` (may be slightly stale
after the K=7 work), `docs/experiments/VERIFIED_FINDINGS.md` (authoritative).

---

## Section 5 - Next-session task ordering

1. **Collect the two bg results** (Section 1 commands). If k7base died
   (OOM/disk), relaunch with capacity <= 30M and log to a file.
2. **Decide on K=7**: if its own LBR is sane and search still helps
   (K7H2H positive), K=7 is the new base. If not, revert to K=6.
3. **5 (depth-limited flop)** and **6 (200bb + Slumbot)** are the only
   large untouched items. 6 needs network.
4. Optional: the review's Leduc CFR grid (8 configs, ~15s each) - do NOT
   change defaults on it (reviewer said so).
5. Optional perf: pad regret record to 64B (`RM_STRIDE=8`), packed flush
   sort key - UNMEASURED, benchmark first.

---

## Section 6 - Environment hazards (unchanged + new)

- macOS: no `timeout` (`gtimeout`), no `awk match(s,re,arr)`, `pgrep -c`
  absent.
- Foreign processes drive load to 100-160 on 8 cores; runs 2-3x slow.
- MEMORY is the real limit: trainer at cap 30M died silently once (OOM).
  Use cap <= 20M for smoke, 60M only when the machine is quiet.
- DISK tight (~20 GiB). K=7 ckpts are ~17% bigger than K=6.
- Always log full trainer output to a file; a bare `--fresh` run that
  dies gives no clue otherwise.
- `.git/index.lock` contention from the watcher is routine; commits land
  anyway. `rm -f .git/index.lock` when no `git` process is live.
- Heredocs frequently break under the multi-watcher setup (content leaks
  into the shell). Prefer Python `open().write()` or `printf '%s\n'`.
- The watcher occasionally re-runs an OLD block: after any file write,
  VERIFY with `grep`/`ls` that it landed as intended.

---

## Section 7 - If you only do one thing

`grep -E "K7BASE DONE|mbb_per_hand" /tmp/k7base-result.txt` and
`grep -E "K7H2H DONE|diff" /tmp/k7h2h-result.txt`. Apply the decision rule
(plan §3): ship a change only if LBR down >= 2*SE AND H2H positive >= 2*SE.

---

**END OF HANDOFF**
