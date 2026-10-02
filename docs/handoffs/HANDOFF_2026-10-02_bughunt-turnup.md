# HANDOFF — bug hunt + turn-up resolution + seed-43 A/B (2026-10-02)

**Continues:** `docs/handoffs/HANDOFF_2026-10-01_bughunt-followup.md`
**HEAD:** see `git log -1`

## Section 1 — Running at handoff

Three jobs, launched via mkdir-lock (see Section 5):
- **turnup eval**: v42 3M blueprint @ 20k -> `/tmp/turnup-test-result.txt`
- **v47** `outputs/v47-p2-s43/`: avg_power=2, seed 43
- **v48** `outputs/v48-p1-s43/`: avg_power=1, seed 43

**Decision when v47/v48 finish:** seed-42 gave p=2=3313.4, p=1=3231.8
(-81.5, inside +/-260 -> keep p=2 by rule; but 6/6 paired favored p=1,
mean -124). If seed-43 reproduces -> ship PKR_AVG_POWER=1; if not ->
seed noise, keep default. Either way ~100 mbb: marginal.

## Section 2 — Turn-up is a measurement artifact

v42 18M checkpoint, same eval, varying deals:

| deals | expl_mbb |
|---|---|
| 5,000 | 3796 |
| 20,000 | 1707 |
| 40,000 | 1222 |

Still falling at 40k -> no absolute number in the docs is converged;
all are upper bounds. Does not fit c/deals. The turn-up is the
5000-deal in-sample estimator, not the policy. Paired A/Bs remain
valid. See `docs/experiments/turn-up-investigation.md`.

## Section 3 — Bugs fixed (6)

| # | where | bug | commit |
|---|---|---|---|
| 1 | reader.rs (new) | fingerprint used infoset_count not preflop_k | 0deece7 |
| 2 | reader.rs (new) | max_actions_k unchecked -> OOB stride | 0deece7 |
| 3 | exploit/lib.rs | shifter read cumulative cdf as raw weights | 690e6ef |
| 4 | writer.rs | purify guard counted p>0.0, collapsed 3-live splits | 034ba82 |
| 5 | state.rs | stale comment claimed fixed C1.5 bug was live | c6be7f3 |
| 6 | main.rs | stats.json omitted PKR_CENTROID_FEATURE_V | a190097 |

## Section 4 — F4 / F5 conclusions

- **F4** (potential features): equivalent (v45 3302 vs v42 3313).
  Keep legacy. No turn rebuild.
- **F5 floor**: RM+ helps on Kuhn; kept.
- **F5 site**: equivalent (v43).
- **F5 avg_power**: Kuhn says p=2; NLHE says p=1 mildly. Seed-43 test
  in flight (v47/v48).
- The `avg_power` axis was never actually grid-tested before this
  session (`kuhn.rs` ignored it); doc corrected.

## Section 5 — Environment hazards

1. **Multi-watcher re-execution.** Every `wr1.sh` write is run by
   several watchexec instances, so any launcher in a watched script
   fires N times and corrupts output dirs. Fix: launch via
   `if mkdir /tmp/X.lock; then ... ; fi` — mkdir is atomic, so exactly
   one run launches. Used for v47/v48.
2. **Heredoc duplication.** Large `cat >> file << EOF` blocks can run
   twice. Fix: write the whole file with `>` (truncate), which is
   idempotent.
3. **`PKR_CENTROID_FEATURE_V` must be exported** to match the training
   run, or the fingerprint guard rejects the checkpoint (v45 arena
   watcher failed on exactly this).

## Section 6 — If you do one thing

Check `pgrep -f pkr-trainer` and `cat /tmp/v47-ab-result.txt`. If
v47/v48 are done, apply the Section 1 decision. If the turnup eval
produced a number, note it next to the 40k table.
