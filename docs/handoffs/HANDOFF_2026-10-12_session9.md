# HANDOFF - 2026-10-12 (session 9: K=7 shipped + audit closed + run 1)

**Continues:** `archive/HANDOFF_2026-10-10_session7.md` (K=7 landed).
**HEAD at handoff:** `28eb914` (main, 2 commits ahead of origin).
**Authoritative plan status:** `docs/roadmap/PLAN_STATUS_2026-10-10.md`.
**Authoritative perf status:** `docs/experiments/perf-P5-P18-2026-10-11.md`.

---

## Section 1 - State of the repo

| | |
|---|---|
| HEAD | `28eb914` (2 ahead of `origin/main`) |
| Working tree | clean |
| Tests | `cargo test --workspace` all green (72 groups) |
| Background jobs | NONE |
| Disk | ~19 GB free |
| Runs on disk | `outputs/k7-baseline/` (SHIPPED), `outputs/run1-combined/` (DO NOT SHIP) |

The session-7 handoff is archived. Everything it said about background
jobs, running trainers, and the K=6/K=7 decision is resolved. Do not
go looking for `/tmp/k7base.lock`, `/tmp/k7h2h.lock` etc. -- none exist.

---

## Section 2 - What landed this session (78 commits)

**Big items (in the order they matter):**

1. **K=7 SHIPPED.** `outputs/k7-baseline/` is the production
   checkpoint. LBR -162.79 +/- 104.49, H2H search-vs-blueprint
   +0.958 (t=+5.28) / +1.279 (t=+7.02). Both plan section 3 gates
   cleared. Full write-up: `docs/experiments/k7-decision-2026-10-10.md`.

2. **M-31 arena refactor of the vector solver.** Removed the last
   `unsafe { &mut *node }` (aliasing UB by the book). `Solver` now owns
   a flat `arena: Vec<Node>` with `u32` child indices. Golden test
   `crates/pkr-subgame/tests/vector_golden.rs` pins `exploitability@50
   = 0.658723` (river) and `exploitability@5 = 26.393711` (turn, chance
   nodes).

3. **M-15 streaming checkpoint loader.** Was: read the whole file into
   one `Vec` (up to 4 GB transient at 60M capacity) then decode. Now:
   1 MB chunks straight into the destination atomics. Peak transient
   ~1 MB. Roundtrip test: `crates/pkr-cfr/tests/checkpoint_roundtrip.rs`.

4. **M-3 strict NLHE min-raise, opt-in.** `PKR_STRICT_MIN_RAISE=1`
   enables the correct short-all-in-does-not-reopen-betting rule.
   Default OFF (the loose game the shipped K=7 ckpt was trained on).
   Strict mode bumps `action_legal_v` 2 -> 3, so strict and loose
   checkpoints are DIFFERENT ABSTRACT GAMES and cannot cross-load.

5. **Phase 4.3 - tracker uses the search policy, not the blueprint.**
   `RuntimeSession::observe_action` now updates our own range with the
   SEARCH probability of the chosen bucket when the search root was
   KEPT WHOLE. `vsearch::solve_root_bounded_traced` returns the
   per-hand matrix. Moot for the current river-only shipping path;
   correct the moment turn/flop search is on.

6. **~50 audit fixes.** Every Critical (C1-C10), every non-deferred
   Major, ~25 Minor. Details in the audit review doc; the class of bug
   (K=6 leftovers breaking K=7) is closed by
   `ci/scripts/check-no-hardcoded-k.sh` plus the shared
   `first_legal_by_bucket` / `sample_legal_index` helpers.

**Perf (P5-P18 triage):** landed P1 (FMph verify-then-use), P10
(signature dispatch cached), P13 partial (1326-combo table cached),
P17 (`madvise(MADV_RANDOM)`); tried and reverted P7 (measurably
slower); verified not-actionable P5/P8/P11/P15/P16.

**Experiments recorded (all in `docs/experiments/`):**
- `k7-decision-2026-10-10.md` -- SHIP K=7.
- `eps-sweep-2026-10-11.md` -- 20M, 6 arms. Lower eps trends better but
  the result is dominated by seed variance; NOT a shipping signal.
- `run1-combined-{2026-10-11,result-2026-10-12}.md` -- see section 3.
- `perf-P5-P18-2026-10-11.md` -- every perf item resolved.
- `phase-5-depth-limited-flop-design.md` -- design + measurement.

---

## Section 3 - Run 1: DO NOT SHIP

Combined `eps=0.002 + PKR_STRICT_MIN_RAISE=1` at 100M, seed 42.

| metric | k7-baseline | run 1 |
|---|---:|---:|
| LBR (20k s=42) | **-162.79 +/- 104.49** | **+98.54 +/- 114.27** |
| H2H search-vs-bp 42 | +0.958 (t=5.28) | +1.105 (t=6.22) |
| H2H search-vs-bp 43 | +1.279 (t=7.02) | +1.078 (t=5.94) |
| in-sample BR @100M | 7922 | 8062 |

The loose baseline re-LBR'd with the CURRENT binary reproduces
byte-for-byte, so the +261 mbb delta is real, not a code drift. Run 1's
in-sample BR is also worse at every eval point. Two independent
measures agree: the combination is a mild regression.

The two knobs were bundled, so this does NOT isolate eps vs strict.
Neither is compelling enough to spend another 12 GPU-hours without a
better reason. **Keep k7-baseline as shipped.**

---

## Section 4 - What's left to do (nothing is urgent)

### Phase 5 (depth-limited flop search)

Design and step-1 measurement are done:
`docs/roadmap/phase-5-depth-limited-flop-design.md`. **Measured
infeasible at the current abstraction resolution** -- the flop tree is
47x the turn tree (14.5 s per build extrapolated to 5,199 hours for
all 1.29M canonical boards). The only viable path is a COARSENED leaf:
512-board table ~12h precompute + wire-up, or 2048-board ~246h. That
is a strength-for-speed trade needing its own A/B.

**Not started. Do not start without a step-1 measurement of the
coarsened leaf first** (the doc says how).

### Phase 6 (200bb + Slumbot)

The 200bb path exists (`PKR_STACK_BB` + fingerprint). The Slumbot
client (`scripts/slumbot_client.py`) exists but the wire format is
UNVERIFIED. This phase needs network. Everything else is ready.

### P9 (UndoRecord slimming)

`GameState` is 3592 bytes; `UndoRecord` (48 x 56 = 2688 B) is 75% of
it. Shrinking `Action.player` -- what the audit claimed -- saves only
192 B, not 800. The real target is `UndoRecord`, but that touches the
on-disk checkpoint format and needs a versioning plan. Deferred.

### Run 2 (isolate strict vs eps)

Two 100M runs to separate the bundled changes. Weak prior from run 1:
both are probably a wash or mildly negative. Only worth it if you have
a specific reason to believe one of them is good.

### Push

`main` is 2 commits ahead of `origin/main` (the run 1 result + this
handoff). `git push` when you want.

---

## Section 5 - Environment hazards (this session's lessons)

**macOS:** no `timeout` (use `gtimeout` if installed, else a watchdog
loop); `awk match()` absent; `pgrep -c` absent.

**Foreign load can drop the trainer to ~1/2 speed.** At one point load
avg was 58 (Zed at 150% CPU). Run 1 took 6h instead of 5.5h. Plan for
it.

**Two failure modes I hit repeatedly:**

1. **Long heredocs get truncated by the file watcher** (6+ times this
   session). A 30-line `cat <<'EOF'` may land only the first 20 lines
   with no error. Mitigations: (a) write to `/tmp/x.py` in chunks via
   `printf`; (b) use `python3 - <<'PYEOF'` with a body shorter than
   ~40 lines; (c) after every write, `wc -l` and `tail -3` to confirm.
   This is the single biggest time sink of the session.

2. **`cargo` alongside a live `pkr-trainer`/`lbr_eval` can OOM-kill
   the run.** It happened twice. Rule: while a trainer or LBR is live,
   do NOT run cargo at all. Read CSVs and grep source instead.

**The `.git/index.lock` is often held by the watcher's own git.** If a
commit fails with the lock, wait 5 s, retry. Do not `rm` it unless
`pgrep -fl 'git '` shows nothing.

**Test temp-file races.** `pkr-abstraction` had two tests writing to
fixed names in `std::env::temp_dir()`; they passed alone and failed in
`--workspace` (parallel binaries). Fixed with `process::id()` suffixes.
If you add a test that touches the filesystem, use `tempfile` or a
per-pid name.

**Disk:** every K=7 checkpoint is ~1.9 GB with a `.prev` rotation
copy. Clean `.prev` and `blueprint.best.bin` after each run.

---

## Section 6 - Where to look for the authoritative word

- Plan progress: `docs/roadmap/PLAN_STATUS_2026-10-10.md`
- Perf: `docs/experiments/perf-P5-P18-2026-10-11.md`
- K=7 decision: `docs/experiments/k7-decision-2026-10-10.md`
- Run 1: `docs/experiments/run1-combined-result-2026-10-12.md`
- Phase 5: `docs/roadmap/phase-5-depth-limited-flop-design.md`
- Environment/repo shape: `docs/status.md`, `docs/INDEX.md`

`docs/experiments/VERIFIED_FINDINGS.md` is authoritative for the K=6
numbers only. K=7 numbers are in the K=7 decision doc.

---

## Section 7 - If you only do one thing

Nothing is urgent. If you want a task:

**Verify the K=7 shipping checkpoint against Slumbot** (Phase 6). It
is the only thing left that measures the bot against anything
external. `scripts/slumbot_client.py` has the protocol; the wire
format is unverified -- run 100 hands against Slumbot's
`sample_api.py` first, confirm the incr tokens match, then 20k hands.
That gives the ONE number the project has never had: real-world
strength.

---

**END OF HANDOFF**
