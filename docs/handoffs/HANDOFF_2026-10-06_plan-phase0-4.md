# HANDOFF — PKR_SOTA_PLAN Phase 0–4 + search findings (2026-10-06)

**Continues:** `docs/handoffs/HANDOFF_2026-10-06_search-and-signature.md`
**Plan:** `PKR_SOTA_PLAN.md` (the authoritative task list)
**HEAD at handoff:** see `git log -1`  **Tree:** see `git status`

---

## Section 1 — Running at handoff

- **V4 vs V5 signature A/B** (`/tmp/overnight-ab-result.txt`): 100M iters
  each, seed 42, preflop-exact ON, one variable (signature). Arm v4
  (`outputs/ab-v4`) running; then arm v5 (`outputs/ab-v5`); then LBR
  both (`--sig 4` / `--sig 5`, 20k hands). ~4-6h total.
- Nothing else running. Disk ~13 GB free (tight; `outputs/` = 3 GB).

### How to read the A/B

`/tmp/overnight-ab-result.txt` ends with `=== complete ===` when done.
Compare the two `LBR ... mbb_per_hand=` lines. **Lower is better.**
- V5 < V4 - 2·SE -> keep V5 (`PKR_SIG_V5=1` default).
- within 2·SE -> keep V4 (fewer moving parts).
- V5 > V4 + 2·SE -> V5 hurts; keep V4.

**Caveat:** both arms are on the NEW preflop-exact key, so this isolates
V5; it does NOT compare to the historical LBR 770.7 (that checkpoint is
`feature_v=0`, unloadable under the current default).

---

## Section 2 — The session's one hard scientific result

**Sampled-hand subgame search is a NET NEGATIVE.** Head-to-head, seats
swapped, same V4 checkpoint: search loses **-6.06 chips/deal, t=-6.26**
(`docs/experiments/search-built-negative-h2h.md`).

Why it matters: LBR had said the OPPOSITE (-627 mbb, t~-5.4). LBR is a
greedy myopic exploiter; it cannot punish an unusual-but-losing policy,
so it scored the bad searched policy as "less exploitable". **LBR alone
is not a strength measure. H2H is.** This is recorded and the sampled
search is parked.

The replacement is the **vector-form solver** (PKR_SOTA_PLAN Appendix A),
now ported: `crates/pkr-subgame/src/vector.rs`. It converges 8.10 -> 0.096
chips over 200 iters on a river spot (real convergence, unlike the
sampled version).

---

## Section 3 — What landed (all committed, all `--all-targets` clean)

**Phase 0 (COMPLETE):**
- **0.1** `PKR_STACK_BB` stack depth; `AbstractionFingerprint.stack_bb_div2`
  (pre-field 0 normalised to 50=100bb). Acceptance verified both ways.
- **0.1 guards** `--fresh` refuses to clobber a >=1MB checkpoint without
  `PKR_FORCE_FRESH=1`; rolling save restores `.prev` if the final rename
  fails.
- **0.2** H2H min-deals guard; `self_h2h_is_exactly_zero` test.
- **0.3** `lbr_eval` emits a JSON result line.
- **0.4** `scripts/slumbot_client.py` (WRITE-ONLY; verify wire format
  against slumbot `sample_api.py` before trusting — needs network).
- **0.5** `docs/experiments/stop-list-2026-10-06.md`.

**Phase 1:**
- **1.1** `crates/pkr-runtime/src/shadow.rs` + wired into `RuntimeSession`
  (`PKR_SHADOW=1`). 15 runtime tests pass.
- **1.2** translation log (`PKR_TRANSLATE_LOG` -> `/tmp/translate.jsonl`).
- **1.3** `crates/pkr-runtime/src/bin/pkr-bot-pipe.rs` — protocol
  SKELETON (parses JSON, hashes via the loaded abstraction, emits "c";
  concrete action emission stubbed pending Slumbot's verified format).

**Phase 2:**
- **2.1** preflop-exact (169 classes) DEFAULT ON (`PKR_PREFLOP_EXACT=0`
  opts out). Fingerprint feature_v=2 by default. Golden hash regenerated.
  **This invalidates every pre-2.1 checkpoint by design.**
- **2.2** V5 signature = V3-minimal + preflop base-7 sequence + postflop
  SPR + prev-aggressor. `crates/pkr-core/src/state.rs::history_signature_v5`.

**Phase 3:**
- **3.1** probability-0 action pruning (`PKR_PRUNE=1`, default off).

**Phase 4:**
- **4.1** `crates/pkr-subgame/src/vector.rs` — the plan's Appendix A
  solver, ported (full 1326 ranges, exact river enumeration, CFR+BR).
  Tests: river convergence, root distribution, index roundtrip.
- **4.1b** `spec_from_state` (GameState -> Spec glue).
- **4.1c** `crates/pkr-runtime/src/vsearch.rs` — `solve_root` driver.
- **4.1d** `RuntimeSession::advise` routes through `advise_vsearch`
  (`PKR_VSEARCH=1`), verified to EXECUTE: blueprint vs vsearch produce
  different river strategies (`docs/experiments/vsearch-executes.md`).

**NOT done:** 2.3 distribution-aware buckets (multi-hour precompute),
2.4 action abstraction (K=6->7 invasive), 4.4 CFR-D gadget (subtle),
5 (depth-limited flop), 6 (200bb build + Slumbot).

---

## Section 4 — The immediate next experiment (highest value)

**Measure whether vector search helps.** It executes; its strength is
unmeasured. Two instruments, BOTH required (LBR alone misled once):

1. **Paired LBR**: `PKR_VSEARCH=1` vs off, same checkpoint, same seed,
   20k hands. `lbr_eval` needs a `--search`-style flag wired to
   `RuntimeSession` (NOT the old `SubgameHandle`; the old `--search`
   path uses the parked sampled solver).
2. **H2H**: the vector search as one side via `tournament --search-a`
   is WIRED to `SubgameHandle`, NOT vsearch — so H2H of vsearch needs
   the tournament to call `RuntimeSession::advise` instead. This is the
   integration gap for the next session.

If BOTH show an improvement (LBR down AND H2H positive), ship search.
If H2H is flat/negative, the vector search (like the sampled one) does
not help at these settings — then 2.3 (buckets) is the remaining lever.

---

## Section 5 — Errors made this session (do not repeat)

1. **Destroyed v42's `train.ckpt`** with `--fresh --checkpoint <real>`,
   and **nearly lost v4's** (resume save left it as `.prev`). Root cause:
   running the trainer against REAL artifacts instead of copies.
   - Guards now cover both, BUT: **always smoke-test the trainer with a
     copy in /tmp**, never a real `outputs/` path.
2. **Many commit messages are jumbled** (the watcher re-runs a block; a
   commit can land with the wrong message, or two `git add` of the same
   file). The CODE is correct; verify with `git show --stat`.
3. **Heredocs double-append** frequently under the multi-watcher setup.
   After any `cat >> file`, check for a duplicated block / duplicate fn.
4. macOS has **no `timeout`** (`gtimeout`), and its `awk` lacks
   `match(s, re, arr)`.

---

## Section 6 — Environment hazards

1. Multiple watchexec instances re-run every `wr1.sh` write. Launch
   long jobs with `if mkdir /tmp/X.lock`. **Never `rm` the lock in the
   same block as `mkdir`** (run 2 removes run 1's lock -> double launch).
2. `.git/index.lock` contention from watcher `git status` — commits
   land anyway; retry with `rm -f .git/index.lock`.
3. `PKR_CENTROID_FEATURE_V`, `PKR_SIG_V3/V4/V5`, `PKR_BUCKET_U16`,
   `PKR_PREFLOP_EXACT`, `PKR_STACK_BB` must match the checkpoint/tables
   or the load aborts (by design).
4. A stale binary predating the u16 field silently trains on garbage.
   **Check `target/release/pkr-trainer` mtime vs the source commits.**

---

## Section 7 — Disk / artifacts

`outputs/` = 3 GB. KEEP: `overnight-v4` (LBR-770 baseline),
`ab-v4`/`ab-v5` (the A/B), `v42-post-audit` (A/B tables), `v34long`,
`v0-smoke`, `v8-fixture` (test deps), `v46-avg-p1` (1 code ref).
DELETED this session: `v42-k400`, `overnight-v3min`, `overnight-k400v4b`
(all recorded in docs).

For a bot smoke: stage `/tmp/botdata` with a v8 ckpt + a full table set
(see `docs/experiments/vsearch-executes.md` for the exact recipe).

---

## Section 8 — If you only do one thing

`cat /tmp/overnight-ab-result.txt`. If `=== complete ===`, apply the
Section 1 decision rule (V4 vs V5). Then start Section 4 (measure the
vector search) — that is the experiment the whole Phase-4 build exists
for.

---

**END OF HANDOFF**
