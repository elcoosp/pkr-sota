# HANDOFF — bug-hunt follow-up + cleanup session (2026-10-01, late)

**Continues:** `docs/HANDOFF_2026-10-01_bughunt.md`
**HEAD at handoff:** `3c967bf`
**Tree:** clean

---

## Section 1 — Running at handoff

**v45 training** (F4 potential-feature A/B vs v42) is STILL RUNNING.
PID 1192, ~15M/30M at handoff, ETA ~1h.

```
outputs/v45-potential/
  best so far: 3392.29 mbb @ 3M   (promoted)
  @ 6M:  3302.32  (skipped, below sigma gate)
  @ 9M:  3500.95  (skipped)
  @ 12M: 3593.68  (skipped)
```

### STALE RESULT FILE HAZARD

`/tmp/v45-result.txt` is **stale and misleading**. It was written at
15:02:58 by a first watcher attempt that ran before `train.pid`
existed, so its wait loop was skipped and the arena step ran against a
non-existent checkpoint. It reads "(parse failed)" and "No such file
or directory".

The live watcher (PID 1333, started 15:07) will overwrite it when
training finishes. Until then, do NOT trust that file. Check
`pgrep -f pkr-trainer` and `outputs/v45-potential/train.log` instead.

### Decision rule when v45 finishes

| v45 best vs v42 best (3313.4 +/- 130) | verdict |
|---|---|
| v45 < 3313.4 - 260 | F4 wins — ship `PKR_CENTROID_FEATURE_V=1` |
| within +/-260 | equivalent — keep legacy |
| v45 > 3313.4 + 260 | F4 hurts — revert |

**Current reading is inside the +/-260 band**, so unless the final third
improves, the rule points at keeping the legacy centroid feature and
skipping the turn rebuild.

---

## Section 2 — This session's actual work

### Bug hunt items 1, 2 (from the prior handoff): NO BUGS

- **`traversal.rs` strategy-batch ordering** — the
  `avg_at_traverser=false` path is correct. The pushed `strategy[a]` is
  the masked opponent strategy, `regret_match_into` on a freshly-created
  infoset returns uniform, and `add_strategy_sum_at` is an atomic add so
  batch order is irrelevant. Noted asymmetry (not a defect): the `false`
  path calls `get_strategy_and_idx` at opponent nodes, so it creates
  opponent infosets the `true` path only creates via the other traverser.
- **`best_response.rs` `opp_reach` threading** — verified correct. At
  BR-seat nodes `e[a] += cv * deal_prior * opp_reach` with `opp_reach`
  unchanged; at opponent nodes the recursion multiplies by
  `strat_masked[a]` and returns `sum strat_masked[a]*cv`. F1 fix is sound.

### Bug hunt item 3: DOCUMENTED BIAS (not fixed)

`potential.rs` `ehs_and_potential`: the per-next-card sampling-variance
estimator `e*(1-e)/inner` is exact for a Bernoulli mean but
*overestimates* `Var(eq)` for the `{0,0.5,1}` equity sample by
`0.25*q` (q = tie probability). Conservative (subtracts too much),
so `potential` is biased slightly low where ties are common. Small on
flop/turn. Documented in the module.

### Bug hunt item 4 + a NEW bug: FIXED

**Item 4 (`tournament.rs` table-dir assumption).** Extracted
`resolve_tables_dir(a, b, explicit)` as a pure, tested helper. The bin
now warns when A and B live in different directories and no `--tables`
was given, because the fingerprint guard only fires when
`PKR_CENTROID_FEATURE_V` is exported to match one side.
Commit `6481207`. Four unit tests.

**NEW bug — `decide_via` fallback diverged from `decide_from_blueprint`.**
`tournament.rs`'s `decide_via` documents that it mirrors the lib's
`decide_from_blueprint`, but its missing-hash path did
"check/call-else-fold" while the lib does a pot-odds/rank decision. On
any blueprint miss, `pkr-arena` and `pkr-tournament` resolved the same
checkpoint to *different* concrete actions — the two F9 tools were not
measuring the same agent. Commit `b624cfe`.

### Bug hunt item 5: DISMISSED

`session.rs` silently swallowing tracker errors — the prior bug-hunt doc
already dismissed this with a sound contract argument (tracker is
advisory; the caller's state is ground truth). Left as-is.

### Cleanup

| commit | what |
|---|---|
| `3c967bf` | silence two example warnings (`save_centroids`, `runouts`) |
| `6481207` | `resolve_tables_dir` + divergence warning |
| `d2fe524` | document the tie bias in `ehs_and_potential` |
| `b624cfe` | align `tournament` hash-miss fallback with `arena` |

**Disk: `outputs/` 5.3 GB -> 2.6 GB.** Deleted seven
recorded-and-unreferenced experiment dirs: `v39rich`, `v40-k250`,
`v43-avg-opp`, `v41-post-f6`, `v33rich`, `v33retest`, `v38`. Each
experiment's result is preserved in `docs/experiments/*.md`. Kept:
`v42-post-audit` (F1 pin + A/B baseline), `v45-potential` (running),
`v34long` (8 test files), `v0-smoke` (CI + 5 test files),
`v44-potential` (F4 flop table, pending decision).

---

## Section 3 — Test state

**327 tests, all passing, 45 skipped.**
`cargo check --workspace --all-targets` -> **0 warnings.**

Note: `cargo check --workspace` does NOT build examples, which is why the
two example warnings survived until now. Use `--all-targets`.

### Known-broken scratch

`wr.sh` still polls `outputs/v38/...`, which was deleted. The `if [ -f ]`
guards make it a harmless no-op. Gitignored scratch; not worth fixing.
The root `launcher-*.sh` scripts were removed between sessions by the
user. Only `scripts/launchers/v42.sh` remains tracked.

---

## Section 4 — What "done" looks like

1. **Wait for v45, decide F4** (Section 1). Check
   `pgrep -f pkr-trainer` first; ignore the stale `/tmp/v45-result.txt`.
2. **If F4 helps:** launch the turn rebuild in the background
   (`pkr-abstraction-precompute turn-potential ...`), ~2.5 days on 8 cores.
3. **If F4 is neutral (current expectation):** keep legacy, skip rebuild.
4. **Optional further disk:** `v44-potential` (328 MB) can go once F4 is
   decided. A hard-link dedup of the shared `turn_abstraction.bin`
   (~1.7 GB across the frozen dirs) is possible but needs a guard that
   refuses to link any dir a launcher can regenerate, because `cp`
   truncates in place and would corrupt every twin.

---

## Section 5 — Process lessons recorded this session

1. **`timeout` is not on macOS** (it's `gtimeout`). A verification step
   using `timeout` silently produced a false negative.
2. **`cargo check --workspace` skips examples.** Verify "zero warnings"
   with `--all-targets`.
3. **A watcher that runs before `train.pid` exists writes a garbage
   result file.** Guard the watcher on the PID file, or wait on the
   trainer's process name rather than a PID file.
4. **Deleted experiment dirs leave stale references.** Grep for
   `outputs/<name>` before deleting; treat only *code* references (not
   doc comments or gitignored scratch) as blockers.
5. **Long heredocs can be truncated when the script file is written in
   pieces.** Verify a written file's closing sentinel before trusting or
   committing it.

---

## Section 6 — File map

| path | state |
|---|---|
| `crates/pkr-fuzz/src/tournament.rs` | `resolve_tables_dir` + tests; fallback aligned |
| `crates/pkr-fuzz/src/bin/tournament.rs` | uses resolver, warns on dir divergence |
| `crates/pkr-abstraction/src/potential.rs` | tie-bias documented |
| `crates/pkr-cfr/examples/check_preflop_actions.rs` | unused import removed |
| `crates/pkr-runtime/examples/bot_loop.rs` | unused binding silenced |
| `docs/experiments/v42-vs-v43-ab.md` | F5 closed (both dimensions) |
| `docs/experiments/f4-abstraction-rebuild-plan.md` | F4 plan + v45 progress |
| `docs/HANDOFF_2026-10-01_bughunt.md` | prior handoff |
| `docs/HANDOFF_2026-10-01_bughunt-followup.md` | this file |

### Disk (2.6 GB total)

- `outputs/v42-post-audit/` — F1 pin + A/B baseline (KEEP)
- `outputs/v45-potential/` — v45 training (RUNNING)
- `outputs/v34long/` — 8+ test dependencies (KEEP)
- `outputs/v0-smoke/` — CI + tests (KEEP)
- `outputs/v44-potential/` — F4 flop table (KEEP until F4 decided)
- `outputs/archive/`, `outputs/v31base-essentials/` — tiny (KEEP)

---

## Section 7 — If you only do one thing

**Check `pgrep -f pkr-trainer`.** If it's gone, v45 finished — then
check `outputs/v45-potential/train.log` for the final best reading and
apply the Section 1 decision rule. Ignore `/tmp/v45-result.txt` until
the live watcher has overwritten it.

---

**END OF HANDOFF**
