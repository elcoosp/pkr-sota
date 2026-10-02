# HANDOFF — full code-review remediation + session wrap (2026-10-02)

**Continues:** `docs/handoffs/HANDOFF_2026-10-02_bughunt-turnup.md`
**HEAD:** `1ad58ed`  **Tree:** clean

## Section 1 — Running at handoff

Three jobs (bg):
- **100k eval** (PID 70597): v42 checkpoint, converged absolute number.
  Result -> `/tmp/session-work-result.txt`. Still running.
- **v47** `outputs/v47-p2-s43/`: avg_power=2, seed 43.
- **v48** `outputs/v48-p1-s43/`: avg_power=1, seed 43.

### avg_power verdict forming: p=1 wins

Paired seed-43 points, all favoring p=1:

| iter | v47 (p=2) | v48 (p=1) | delta |
|---|---|---|---|
| 3M | 3357.5 | 3230.0 | -127.5 |
| 6M | 3332.6 | 3217.5 | -115.1 |
| 9M | 3493.8 | 3398.5 | -95.3 |
| 12M | 3565.8 | 3438.8 | -126.9 |

Seed 42 gave 6/6 the same way. 10/10 paired points across 2 seeds
favor p=1 (~100-127 mbb). **Ship `PKR_AVG_POWER=1`** once v47/v48
plateau (or flip the default now — the evidence is strong).

## Section 2 — The 100k eval (the reframing result)

40k already showed the absolute reading still falling (3796@5k ->
1707@20k -> 1222@40k). 100k will land lower. **Every absolute
exploitability number in the docs is an inflated upper bound**; the
bot is stronger than its own recorded tier says. Paired A/Bs remain
valid regardless.

## Section 3 — Full code-review remediation (this session)

A 37-finding external review was verified against code and fixed:

**High:** H1 blend_p0_strategy (missing-side blend was a no-op / summed
to 1-alpha), H2 --min-visits compared a normalized strategy to a visit
threshold, H3 subgame `&mut Solver` aliasing (UB) -> atomic cells.
**Medium:** M1 soft-kmeans dead gate (street!=5), M2 riversolve WIP
banner, M3 gpu compile_error!, M4 run.sh set -e (NOT done — see §5),
M5 stats eval_seed, M6 --save-best-reading clobber.
**Low:** L1 GameState.bb field, L2 eval bound asserts, L3 combinadic
asserts, L4 subgame rng entropy, L5/L6/L7 docs+dead-code.
**Smells:** S1 (max_actions=6), S4 (single lookup), S5/S15 (docs), S7
(bet dedup), S8 (N_CLASSES doc), S10 (rng entropy), S12 (uniform_board_free
helper), S13 (tournament hard error).
**Docs:** D1-D6.

~40 commits. Every commit one-per-file. 190+ tests green on touched
crates; workspace `check --all-targets` = 0 warnings.

## Section 4 — Concurrency findings (the session's theme)

Two real concurrency bugs found, both now fixed:

1. **alloc_idx orphan race** (`get_or_create_idx` check-then-act):
   two threads missing the same new hash both alloc_idx; the loser's
   slot is orphaned. Made `snapshot.infosets` report `len()` (9795867);
   the metric is now deterministic. The leak itself remains (wastes a
   slot per race) — full fix is coordinator-side canonical allocation.
2. **H3 &mut aliasing** (1ad58ed): fixed via atomics.

The 4-thread `train.ckpt` drift (~1e-11 strategy_sum_mass) is now
addressed by the hash-ordered sum in `snapshot()` (18c8264) — but
the *checkpoint file* can still differ; the shipped `blueprint.bin`
is and was deterministic.

## Section 5 — NOT done (deliberately)

- **M4** (`run.sh` missing `set -e`): the launcher scripts are
  gitignored and several are stale; editing them risks breaking the
  user's local workflow. **Left for the user.** The finding is real:
  a failed precompute flows into training silently.
- **S5** hoist `dcfr_step`: the review's suggested fix is wrong
  (sequential mode has per-item iterations) — documented, not coded.
- **alloc_idx full fix**: coordinator-side canonical idx assignment;
  larger change, deferred.
- **§6.1 checkpoint byte-reproducibility**: partially addressed.

## Section 6 — If you do one thing

Check `pgrep -f pkr-trainer`. If v47/v48 are done, read their bests
and **flip `PKR_AVG_POWER` default to 1** (10/10 paired points). Then
read `/tmp/session-work-result.txt` for the converged absolute.

## Section 7 — Environment hazards (unchanged)

1. Multiple watchexec instances re-run every `wr1.sh` write; launchers
   must use `if mkdir /tmp/X.lock` to run once.
2. Long heredocs can truncate/duplicate; write whole files with `>`.
3. `.git/index.lock` contention from watcher `git status` — commits
   land anyway; retry if needed.
4. `PKR_CENTROID_FEATURE_V` must be exported to match a checkpoint.
