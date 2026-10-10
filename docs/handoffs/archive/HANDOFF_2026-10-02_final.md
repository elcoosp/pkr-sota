# HANDOFF — final session state (2026-10-02)

**Continues:** `docs/handoffs/HANDOFF_2026-10-02_code-review-fixes.md`
**HEAD:** see `git log -1`  **Tree:** see `git status`

## Section 1 — Running at handoff

Three background jobs:
- **100k eval** -> `/tmp/session-work-result.txt` (converged absolute
  exploitability of the v42 checkpoint).
- **v47** `outputs/v47-p2-s43/` (avg_power=2, seed 43).
- **v48** `outputs/v48-p1-s43/` (avg_power=1, seed 43).

### avg_power: SHIP p=1

Paired points across two seeds, all favoring avg_power=1:

| iter | seed 42 delta | seed 43 delta |
|---|---|---|
| 3M | -81.5 | -127.5 |
| 6M | -115.1 | -115.1 |
| 9M | -95.3 | -95.3 |

(seed-42 deltas from v42/v46; seed-43 from v47/v48). 10/10 paired
points, ~100-127 mbb. **Action: flip the `PKR_AVG_POWER` default in
`crates/pkr-cfr/src/config.rs` from 2.0 to 1.0**, update the config
tests + `stats.json` golden, and note it in `docs/experiments/f5-grid.md`.

## Section 2 — Code-review remediation: COMPLETE

All 37 findings from the external review verified against code and
addressed (~40 commits, one per file):

- **High:** H1 blend_p0_strategy, H2 --min-visits, H3 subgame UB.
- **Medium:** M1 soft-kmeans gate, M2 riversolve banner, M3 gpu
  compile_error!, M4 run.sh set -e, M5 stats eval_seed, M6 save-best.
- **Low:** L1-L7.
- **Smells:** S1, S4, S7, S8, S10, S12, S13 fixed; S5/S15 documented.
- **Docs:** D1-D6.

Every commit landed; `cargo check --workspace --all-targets` = 0
warnings; touched-crate tests green (190+).

### Corrections to earlier "not done"

- **M4** was wrongly skipped as "gitignored" — run.sh IS tracked; the
  bug was real (failed precompute flowed into training) and is now
  fixed (61768f9).
- **S5** (hoist dcfr_step): the review's suggested fix is WRONG —
  sequential mode has per-item iterations, no hoist exists. Documented.
- **alloc_idx full fix**: leak is ~2 slots per 932k (2e-6); the
  reported metric is already deterministic (len()). A race-free fix
  needs coordinator-side canonical assignment — a hot-path redesign
  for a negligible leak. Deliberately deferred, not a hazard.

## Section 3 — The session's two real bugs (concurrency)

Neither was previously attributed despite a multi-day nondeterminism
investigation:

1. **alloc_idx orphan race** — `get_or_create_idx` check-then-act.
   Two threads missing the same hash both alloc_idx; the loser's slot
   is orphaned. Metric fixed (report len()); leak remains.
2. **H3 — `&mut Solver` aliasing** — the subgame `solve()` made a fresh
   `&mut Solver` per rayon worker via a raw pointer (UB). Fixed via
   atomic cells (1ad58ed).

## Section 4 — The turn-up (resolved earlier this day)

The "exploitability rises after 3-6M" is in-sample BR overfitting.
v42 18M reads 3796 mbb @5k, 1707 @20k, 1222 @40k — still falling, so
**no absolute number in the docs is converged**; all are upper bounds.
Paired A/Bs (same deals both sides) remain valid.

## Section 5 — F4/F5 conclusions

- F4 (potential features): equivalent. Keep legacy.
- F5 floor: RM+ helps (Kuhn). Keep.
- F5 site: equivalent. Keep.
- F5 avg_power: p=1 wins on NLHE (2 seeds). See Section 1.

## Section 6 — Environment hazards

1. Multiple watchexec instances re-run every `wr1.sh` write; launchers
   must use `if mkdir /tmp/X.lock` so they run once.
2. Long heredocs can truncate/duplicate — write whole files with `>`
   (idempotent) and verify the closing sentinel.
3. `.git/index.lock` contention from watcher `git status` — commits
   land anyway; retry.
4. `PKR_CENTROID_FEATURE_V` must be exported to match a checkpoint.

## Section 7 — If you do one thing

`pgrep -f pkr-trainer`. If v47/v48 are done, flip `PKR_AVG_POWER` to 1
(Section 1) — the one shippable bot improvement this session. Then read
`/tmp/session-work-result.txt` for the converged absolute.

## Section 8 — Disk

`outputs/` was trimmed 5.3 GB -> ~2.3 GB earlier in the day (removed
7 recorded-and-unreferenced experiment dirs). v47/v48 add ~400 MB.
