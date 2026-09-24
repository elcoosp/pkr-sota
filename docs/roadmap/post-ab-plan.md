# Post-A/B roadmap: from current tier to production-grade

**Status at time of writing:** overnight A/Bs running (v29alt, v29rb250,
v29t22, v29fb250, v29hs, v29a2). Current best known config: α=1.5,
ε=0.01, k=200, all-flags-off. Abstract-space exploitability ~5500 mbb
at 200M. Real-game likely 15,000-30,000 mbb.

**Honest tier assessment:** weak-bot. Beatable by any competent HUNL
player. Not competitive; potentially useful as a sparring partner or
as an infrastructure base for future work.

---

## Part 1 — Decision tree (depends on overnight A/B)

Once `/tmp/overnight-v2.log` has results, pick exactly one branch:

### Branch A — At least one flag wins big (< 5100 mbb at 200M)
**Meaning:** the algorithm has headroom we hadn't exploited.
- Promote the winning flag to default in code (remove env gate).
- Re-run the other flags on top of the winner (interactions matter).
- Budget: 1 week to lock in, then productionize.
- Ceiling estimate: competent-bot tier (< 2000 mbb) within 2-4 weeks.

### Branch B — All flags within noise, best is ~5300-5600
**Meaning:** the k=200 abstraction ceiling is binding.
- Accept the current tier.
- Either (B1) productionize as-is, or (B2) invest in a structural
  change: continuous features / a bigger abstraction with more
  compute.
- Budget: 1 week for B1, 4-8 weeks for B2.
- Ceiling estimate: weak-bot tier (current) for B1; competent for B2.

### Branch C — All flags worse than baseline
**Meaning:** the A/Bs tested the wrong axes OR there's a measurement
problem we missed.
- Kill the current line of work.
- Re-audit: eval methodology, table freshness, blueprint loading.
- Budget: 3-5 days of investigation.

---

## Part 2 — Production-grade checklist

Independent of A/B results. These make the codebase ship-able rather
than make the strategy stronger.

### P1 — Runtime (what the host app links against)
- [ ] Freeze the blueprint format. Bump to v5 with a documented
      migration path from v4. Old blueprints must load or fail with a
      clear error.
- [ ] `SolverHandle` currently exposes `get_advice_fast` at ~13ns p99.
      Add `get_advice_batch(&[u64])` for callers with many queries.
- [ ] Add `--health` mode that does a synthetic 100-query loop and
      reports latency distribution. Host apps can self-check.
- [ ] Replace `panic!` on bad blueprint with a `Result`. The runtime
      is embedded in long-running hosts; panics kill the process.
- [ ] Document the memory-mapping contract: file must stay
      unchanged while the handle is alive. Currently implicit.

### P2 — Reproducibility and CI
- [ ] `--seed` already exists. Add `--seed` to `stats.json` output.
- [ ] CI job that runs `--iterations 100000 --seed 42` and asserts
      the resulting `train.ckpt` hash matches a golden value. Catches
      silent behavior drift.
- [ ] CI job that rebuilds the smoke blueprint and runs
      `load_external_blueprint`. Catches format mismatches.
- [ ] Nightly job that runs the full smoke + 10M-iter training, records
      `metrics.csv`, alerts if it/s drops more than 20% day-over-day.

### P3 — Evaluation integrity
- [ ] AIVAT-style variance reduction. Real technique (Burch et al.
      2018); I don't have a verified 2026 reference. Implement from
      the algorithm description, not from a paper I can't check.
      Effect: 3-10× SE reduction on the exploitability estimate.
      This is the highest-leverage production change because every
      future A/B becomes cheaper to run.
- [ ] Report both in-sample and held-out (post-C1 fix) exploitability
      side by side. The gap is a sanity check on the BR fit.
- [ ] Log the exploitability standard error in the CSV (already
      present) and refuse to promote if the improvement is < 2σ.

### P4 — Observability
- [ ] Structured logs (JSON) with iteration, infosets, it/s, cache
      hit, regret max, nonfinite, and any env flags active.
- [ ] `stats.json` should record every env flag's value (currently
      only some are captured). Full config reproducibility.
- [ ] Metrics endpoint for hosts: infoset count, blueprint age, load
      time, per-street advice latency.

### P5 — Documentation
- [ ] `README.md` claim audit: verify every number is reproducible
      from a clean checkout. Delete claims that can't be reproduced.
- [ ] `docs/HANDOFF.md` — this session's handoff doc is stale; write
      a fresh one. (The overnight-v2 script will leave a good log;
      that plus the codebase state is the new handoff.)
- [ ] `docs/CHANGELOG.md` — I've been appending but the changelog
      is now a running log, not a release log. Restructure into
      releases (v0.1, v0.2, ...) with a "what changed" summary per
      release.

### P6 — Anti-exploitation
- [ ] Add a "opponent-model check" — play against a fixed scripted bot
      (fold-heavy, call-heavy, jam-heavy) and confirm the blueprint
      doesn't lose to any of them by more than expected. Catches
      pathological blueprints.
- [ ] Add a "self-play sanity" test — a blueprint should not lose to
      itself by more than noise. Catches BR-walker regressions.
- [ ] Rate-limit + log any blueprint that has `uniform_fallback > 5%`.
      This was flagged in the earlier audit and is a real production
      risk (uniform play at many infosets).

---

## Part 3 — Competitiveness path (algorithmic)

Ordered by (expected gain / implementation cost). Do not start any
of these until Branch A/B/C is decided.

### C1 — AIVAT for evaluation  [high value, medium cost, ~3 days]
Reduces eval SE 3-10×. Makes every future experiment 10× cheaper.
**Not a strategy improvement** — a measurement improvement.
Enables everything else to be tested faster.
Requires: EHS heuristic value function, frozen before eval.

### C2 — Alternating updates  [potentially large, structural, ~1 week]
Already implemented (`PKR_ALT_UPDATES=1`), pending A/B.
If it wins, promotes to default.
If it loses on this game, document the negative result and move on.

### C3 — Higher-resolution abstraction on ONE axis  [medium, ~1 week]
The T2.2 river test showed finer river buckets don't help at k=200.
Try the same on flop buckets (k=250) or turn centroid count.
Overnight A/B will answer this for k=250.

### C4 — Continuous features instead of discrete buckets  [large, ~4-8 weeks]
Replaces k-means bucketing with a learned or designed continuous
encoding. The real fix for the abstraction ceiling — but a substantial
project. Real techniques in this space (from my training data, not
verified for 2026):
- Neural network feature extractors trained on showdown equity
- Randomized clustering ensembles (soft buckets)
- Rank-based raw features with a bigger tree (no abstraction at all
  for a smaller game like short-stack HUNL)

Not recommended until C1-C3 are exhausted.

### C5 — More compute  [large, hardware cost]
Pluribus trained on 128 CPUs for days. We have 8 cores.
An AWS spot instance with 64 vCPUs, run for a week, would buy us
roughly the same compute as 2 months on the M1.
Cost: ~$300-500 for a serious run.
Requires: cloud-portable build (no M1-specific code), checkpoint
transfer, orchestration.
This is the honest answer for "how do we catch up" — we can't, on
local hardware. We need either smarter algorithms (C1-C4) or more
compute (C5).

### C6 — Hybrid: off-tree subgame solving  [research, multi-week]
DeepStack/Libratus approach. Requires:
- A value network trained on self-play
- A river (or turn+river) subgame solver
- Integration with the blueprint at inference time
This is the algorithm family that beat top humans in 2017. It is
still SOTA-ish but not the frontier in 2026 (from what I can
verify). Realistic only with committed engineering time.

### C7 — Wait for public code  [zero effort, uncertain payoff]
Poker AI research groups occasionally release code. If a paper
matching our architecture (CFR + abstraction + 1-machine trainable)
appears with code, port it. Requires monitoring for such releases
and having the codebase clean enough to integrate.

---

## Part 4 — Recommended sequence

**Immediate (this week):**
1. Read overnight A/B results. Pick Branch A/B/C.
2. If Branch A: promote winning flag, re-run subordinates.
3. If Branch B: accept current tier; start P1-P6 checklist.
4. Either way: implement AIVAT (C1). It amortizes across everything.

**Short term (next 2 weeks):**
5. Finish production checklist. The bot becomes embeddable.
6. Run C2 (ALT) and C3 (k=250) confirmation at 500M iters each.
7. Write a truthful README: "weak-bot tier, XX mbb, trained on X
   iterations, requires Y to reach competent tier."

**Medium term (next 2 months):**
8. Decide between C4 (continuous features) and C5 (more compute).
   C5 is cheaper if you can rent compute. C4 is more intellectually
   interesting but uncertain.
9. If pursuing C5: port to cloud, run 4-week training, evaluate.

**Long term (next 6 months):**
10. If competitive tier is the goal, C5 or C6 is required.
11. If production-tier with current strength is the goal, ship.

---

## Part 5 — What we will NOT do

- **Finer discrete buckets (river, turn, flop):** T2.2 proved no
  headroom at k=200-400. The overnight k=250 tests are the last word.
- **Neural CFR variants (Deep CFR, ReBeL):** no infra, no clear
  code release, uncertain payoff on 1 machine.
- **Self-play RL:** different research direction, no clear win over
  CFR for HUNL.
- **Hand-tuned heuristics on top of the blueprint:** would only mask
  the abstraction ceiling, not fix it.
- **More A/Bs of the same flags at 20M iterations:** the overnight
  runs are 200M. Anything less is noise.

---

## Part 6 — Success metrics

For each phase, the acceptance threshold:

| phase | metric | target |
|-------|--------|--------|
| Overnight A/B | best run vs v25final @ 200M | ≤ 5100 mbb = real win |
| Production | runtime latency p99 | < 1 ms |
| Production | blueprint load time | < 100 ms |
| Production | `uniform_fallback` rate | < 1% |
| Production | CI test pass rate | 100% |
| Competitiveness | abstract-space exploitability | < 2000 mbb |
| Competitiveness | real-game estimated exploitability | < 5000 mbb |
| Competitiveness | loses to scripted bots | 0 of 5 |

The last row is the most honest test: "does a well-known bad
opponent exploit us?" If yes, we're not competitive.

---

## Summary

The overnight A/B is the pivot. Everything downstream depends on
which branch it lands in.

If it lands in Branch B (most likely, given 20M results), the honest
path is: **productionize the current bot at weak-bot tier**, then
decide whether to pursue the multi-month algorithmic/compute work
needed for real competitiveness.

If it lands in Branch A, we have real algorithmic headroom, and a
few weeks of focused work can plausibly reach competent tier.
