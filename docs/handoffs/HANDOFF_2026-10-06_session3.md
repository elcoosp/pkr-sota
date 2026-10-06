# HANDOFF — session 3 (2026-10-06 evening)

**Continues:** `docs/handoffs/HANDOFF_2026-10-06_plan-phase0-4.md`
**Plan:** the user-supplied `PKR_SOTA_PLAN.md` (in the chat, not on disk)
**HEAD at handoff:** `git log -1`  **Tree:** `git status`

---

## Section 1 — What this session did

Session 2 closed the handoff-§4 integration gap (wire vector search into
LBR + tournament) and armed the measurement. This session did that
plus a deep bug hunt, ran the measurement infra through tiny smoke
tests, and fixed 10 real bugs.

### Landed (all committed, all tests pass)

**Search integration (from session 2, in `git log`):**
- `SearchHandle` trait unifying `SubgameHandle` (sampled) and
  `VSearchHandle` (vector).
- `--search-vector` on `lbr_eval` and `tournament`; `VSearchCfg.enabled_streets`
  street gate; `--search-streets` on both.

**Bug hunt — 10 fixed:**

| # | Where | Bug | Commit |
|---|---|---|---|
| 1 | `vsearch.rs`, `session.rs` | flop solve hang (no street gate in `solve_root`; `advise` used `!= Preflop` as proxy) | `a7c5b44` |
| 2 | `tournament.rs` | no `--sig` flag | `777f152` |
| 3 | `arena.rs` | no `--sig` flag | `9889552` |
| 4 | `expl_eval.rs` | no `--sig` flag | `d2852d3` |
| 5 | `pkr-bot-pipe.rs` | `--sig` parsed to `_sig`, never applied | `d2852d3` |
| 6 | `vsearch.rs` | `decide` accepted empty masked opp range → uniform | `0844d47` |
| 7 | `session.rs` | our own actions ran through randomized translation | `04c9c11` |
| 8 | `shadow.rs` | `observe_own_bucket` fallback folded | `0fd52a8` |
| 9 | `lbr.rs` | hook got unmasked opp range (BR path got masked) | `68e7377` |
| 10 | `tournament.rs`/`arena.rs`/`expl_eval.rs` | `--sig` default clobbered caller's `PKR_SIG_*` env | `fef8098` |

Bug 10 was a regression I introduced myself with bugs 2–4; the smoke
test caught it before the queued measurement ran.

**Instrumentation:**
- `vsearch::VSEARCH_SOLVE_COUNT` / `VSEARCH_GATE_REFUSED` atomics.
- `PKR_SEARCH_DEBUG=1` prints the counters in `lbr_eval` / `tournament`.

**Docs:**
- `docs/experiments/bughunt-2026-10-06-session2.md` (9 bugs).
- `docs/experiments/vsearch-measurement-wiring.md` (integration).
- `docs/experiments/vsearch-smoke-2026-10-06.md` (pre-measurement).

---

## Section 2 — Running at handoff

**V4 vs V5 A/B** (`/tmp/overnight-ab-result.txt`): arm v4 at 95M/100M
(ETA ~10 min), then arm v5 (~4 h), then LBR both. The v5 dir does not
exist yet.

**Queued measurement** (`/tmp/vsearch-measure2.sh`, PID tracked):
polls for `=== complete ===` in the A/B result, waits 60s, then runs
4 instruments against `outputs/ab-v4/train.ckpt`:
0. plumbing sanity (`--search-vector --search-streets n`, 2k hands)
1a. LBR blueprint-only (20k, 400 samples)
1b. LBR `--search-vector --search-streets r --search-iters 100`
2. H2H `--search-a --search-vector --search-streets r`
Output: `outputs/vsearch-measurement/`, `/tmp/vsearch-measure2-result.txt`.

---

## Section 3 — Preliminary smoke result (NOT the measurement)

200-hand LBR, seed 42, 400 samples:

| arm | mbb/hand | SE |
|---|---|---|
| blueprint-only | -713.75 | 876.5 |
| search, 100 iters | -1450.00 | 955.8 |

Difference -736 mbb, |t| ≈ 0.6 at 200 hands. **This is the same
direction the sampled solver showed on LBR** — and the sampled solver
then LOST the H2H (-6.06 chips/deal). Do not read this as "search
helps"; wait for the 20k + H2H numbers.

Search converges by 30 iters (30 and 100 identical).

---

## Section 4 — Decision rule (unchanged)

Ship the vector search only if BOTH:
- LBR `search < blueprint − 2·SE`, AND
- H2H `mean_diff(A=search − B=blueprint) > +2·SE`.

If LBR improves but H2H is flat/negative → park (sampled-solver fate).
If both flat → next lever is 2.3 (distribution-aware buckets).

**Regardless of the outcome, the raw vector solve has no safety bound
(no CFR-D gadget). A positive H2H is necessary but not sufficient to
ship to a real opponent; add the gadget first.**

---

## Section 5 — Environment notes

- macOS: no `timeout`/`gtimeout`; no `awk match(s,re,arr)`.
- The `.git/index.lock` contention from the watcher is frequent; commits
  land anyway. `rm -f .git/index.lock` if stale (check no `git` process
  first).
- Foreign heavy load observed (`glyim-v2` recursive `cargo-llvm-cov`,
  `chameleon`) — not ours, left alone.
- The A/B launcher `/tmp/overnight-ab.sh` runs v4 then v5 back to back.

---

## Section 6 — If you only do one thing

`cat /tmp/vsearch-measure2-result.txt`. If it ends with `ALL DONE`,
apply Section 4. If the A/B has not finished, wait.

Then: **bug 1's fix is load-bearing for the whole measurement.** A flop
decision with `PKR_VSEARCH=1` used to hang. Verify the flop-gate fix is
in HEAD (`grep "enabled_streets\[st.street" crates/pkr-runtime/src/vsearch.rs`)
before trusting any run that reaches a flop with search on.
