# SOTA candidate techniques — honest inventory

**IMPORTANT: The agent that wrote the earlier version of this file claimed to
have done web research. It had no web access. Those citations were
fabricated and have been removed. Everything below is from an LLM's training
data and has NOT been verified against the 2026 state of the art. Before
implementing any of these, a human with browser access must verify:**

- The technique still exists in the literature
- It has released code (many don't)
- Its reported results are on a comparable game scale

---

## Real techniques I can name from training data (unverified for 2026)

### A. Techniques we already use
- **CFR+** (Tammelin 2014) — regret matching+ with floor at 0
- **DCFR** (Brown & Sandholm 2019) — discounted CFR; we use α=1.5, β=0, γ=2 via `PKR_AVG_POWER=2`
- **MCCFR** (Lanctot 2009) — external sampling; our `traverse` is external-sampling MCCFR
- **Linear CFR** (Brown & Sandholm 2018) — averaging with linear weight; we do `t^2` (DCFR γ=2)

### B. Techniques we tried and disabled
- **PCFR+ momentum** (Farina et al. 2021) — we disabled it (`PKR_MOMENTUM=0`) because the update was adding the prediction to regret instead of the true increment. A correctly-implemented PCFR+ might work; not tried cleanly.

### C. Techniques we have NOT tried but which exist in the literature

1. **Alternating updates** — CFR+ prescribes alternating player updates with fresh strategies for the second player. Our `traverse` does both players per iteration from one RNG stream. **Worth checking**: does our code actually alternate? If not, might be a cheap quality win.

2. **AIVAT** (Burch, Schmid, Moravčík, Morill, Bowling 2018) — variance-reduced exploitability evaluation. Uses hand-strength expectations to construct a control variate. Reported 4-10× SE reduction. Pure-eval technique; no training change. **Worth implementing** for every future A/B.

3. **Discounted discount sweep** — we've fixed α=1.5, β=0, γ=2. Smaller α (e.g. 1.0 = Linear CFR-like) or larger (2.0) shifts the average-strategy recency weighting. Not tried. One-line change.

4. **Warm-start from a checkpoint with a fresh discount epoch** — resume v25final's checkpoint with a new iteration counter, effectively treating the existing regrets as a warm start. Cheap test.

5. **Signature v2** — `SIG_V2_STREET_MONEY=true` distinguishes "facing 0.5× pot" from "facing 2× pot". Already in code as a compile-time const, currently `false`. ~6× infoset keyspace. Never tested.

6. **Extra bet sizing** — currently {0.5, 1.0, 2.0}× pot. Adding a fourth size (e.g. 0.33× or 3.0×) or replacing with a geometric ladder. Action abstraction change, not tested.

7. **Turn abstraction refinement** — T2.2 refined river (failed). Turn is the other axis. Higher infoset count, may have the same failure mode; but worth one A/B.

8. **Off-tree lookahead / subgame resolving** — the Libratus/DeepStack approach. Large architectural change; not viable in a session.

9. **Neural CFR variants (Deep CFR, ReBeL)** — need NN infrastructure; out of scope.

### D. Techniques we should stop pursuing
- **Finer river discretization** — T2.2 proved the discrete-abstraction axis has no headroom at this iteration count.
- **PCFR+ momentum with schedule tuning** — the update was structurally wrong, not a schedule problem.

---

## Concrete next experiments, ranked by (expected gain / effort)

| # | Technique | Effort | Risk | Independent of |
|---|-----------|--------|------|----------------|
| 1 | Alternating updates check | 10 min read | Low | Everything |
| 2 | Discount α sweep at 20M × 3 | 1h | Low | ε A/B (parallel) |
| 3 | AIVAT eval implementation | Half day | Medium | All future A/B |
| 4 | Signature v2 A/B | 30 min | Medium (HASH) | Nothing |
| 5 | 4th bet sizing | 30 min | Medium (HASH) | Nothing |
| 6 | Warm-start from v25final | 20 min | Low | Nothing |
| 7 | Turn abstraction | 2h | Medium | Nothing |

---

## What must be verified externally before acting

Before writing any code that follows these papers, a human with web access
should look up:

- **DCFR** (Brown & Sandholm): the α, β, γ we use and whether later papers
  found better values.
- **AIVAT** (Burch et al.): the exact algorithm; is there a Rust port?
- **Discounted CFR variants** post-2019: has the schedule changed?
- **"Alternating updates"** in modern CFR variants: is it still standard?
- Any 2024-2026 papers on **abstraction for HU NLHE** that don't fall into
  the "finer discrete buckets" trap we already proved is a dead end.

Do not skip this step.
