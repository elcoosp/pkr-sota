# SOTA research needed — no fabricated citations

**Status: pending real research.**

An earlier version of this file (written 2026-09-24) contained specific
paper titles, arXiv IDs, and GitHub URLs that were **fabricated by an
LLM that had no web access**. That version has been deleted.

## What we need from a human with web access

1. **Search Google Scholar / arXiv** for 2024-2026 papers on:
   - CFR variants (CFR+, DCFR, PCFR+, MCCFR, Linear CFR)
   - Poker abstraction techniques (bucketing, embeddings, imperfect
     recall)
   - Variance reduction for game-solving evaluation (AIVAT and
     successors)
   - Deep learning + CFR hybrids (Deep CFR, ReBeL, neural CFR)

2. **For each candidate:** verify the paper exists, has released code,
   and reports results on a game size comparable to HU NLHE.

3. **Bring back 2-3 concrete techniques** with real citations. Write
   them here with verified links.

## What we can say without web access

From training data (real, but possibly stale vs 2026):

- **AIVAT** (Burch, Schmid, Moravcik, Morill, Bowling) — variance
  reduction for poker evaluation. Real. Reduces exploitability SE by
  roughly 5-10x in published work. Worth implementing.
- **CFR+** (Tammelin) — regret matching with alternating updates.
- **DCFR** (Brown & Sandholm) — discounted CFR, which we already use
  via PKR_AVG_POWER.
- **PCFR+** (Farina et al) — predictive CFR+, our momentum path.
- **ReBeL** (Brown et al) — recursive belief + CFR. Requires NN.
- **Deep CFR** (Brown et al) — NN function approximation of regrets.
  Requires significant infra.

## Concrete next step

Before writing any implementation plan that cites papers, get the
citations verified by someone with web access. The 2026 state of the
art may include techniques not represented in my training data.
