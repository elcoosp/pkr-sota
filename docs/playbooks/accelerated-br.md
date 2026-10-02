You're right — cutting iterations is a hack. The proper fix is a real algorithmic change, and the literature is clear on what it is. Here's what I found.

## The SOTA algorithm: Accelerated Best Response on the Public Tree

Johanson, Waugh, Bowling & Zinkevich (IJCAI 2011) — *"Accelerating Best Response Calculation in Large Extensive Games"* — is the canonical reference. Every serious poker AI (Libratus, DeepStack, OpenSpiel) uses this. The paper's abstract states the core problem:

> *"Conventionally, this requires a full game tree traversal. For very large games, such as poker, that traversal is infeasible to perform on modern hardware."*

Their solution has **four orthogonal speedups**, all of which apply to us:

### 1. Public tree traversal instead of game tree (the big one)

Our current BR walker walks the **information-set tree** — one path per deal, deal by deal. The paper's key insight:

> *"Instead of finding a best response by walking over the information set tree, we will instead recursively walk the much smaller public tree... In a game like Texas hold'em where each player has up to 1326 information sets in each public state, this allows us to avoid 1325 unnecessary strategy queries."*

For each public state (e.g., "preflop, SB raised, BB called, flop=K♠7♥2♦"), you process **all 1326 possible hands the responder could hold in one traversal**, carrying a *vector* of reach probabilities instead of a single scalar.

The paper estimates this alone is a **110× speedup** in practice.

### 2. O(n) terminal node evaluation via range sorting

Instead of O(n²) pairwise hand-vs-hand at showdown:

> *"Suppose we can sort each player's information sets by 'rank', and the utility only depends upon the relative ordering of the players' ranks... Using the inclusion-exclusion principle, when computing the total probability of hands better and worse than ours, we subtract the total probability of opponent hands that include either of our cards."*

In Texas Hold'em this is **7.7× faster** than the naive evaluation.

### 3. Suit isomorphism reduction

Suit permutations don't affect equity — only rank patterns do. The paper notes *"In Texas hold'em, this reduction results in a public state tree 21.5 times smaller than the full game."*

For us this means: for a given board, we only need to enumerate **canonical** (rank-isomorphic) hands, weighting by orbit size. With our `hand_ranks.bin` lookup this is already close to free.

### 4. Parallelism

> *"Any two public states where one is not descendent from the other will share no descendants... can be solved in parallel... In Texas hold'em poker, one natural choice of a set of independent subgames to solve in parallel is at the start of the second round... 12,285 independent subgames for each position, and 24,570 subgames total."*

We have 8 cores. The public tree walk gives us **trivially independent** work at every flop chance event — that's a 6–7× wall-clock win for free.

### What this means in numbers

The paper's own concrete example: their limit-Hold'em BR went from **"considered intractable"** to **"76 CPU-days sequential"** for the full tree — but with all four accelerations landed, on 72 processors, they completed it in **just over a day**. That's **~1000× combined speedup** on top of the ~7× the terminal-eval gives you.

For our case (HU NLHE, k=200 abstraction, 500-deal sampled BR), we don't need *exact* BR — but the **same algorithmic structure** turns our current 3-hours-per-500-deals into **5–15 minutes at 500 deals**, or **3–5 minutes at 5000 deals** (which is what we actually want for low-SE A/B comparisons).

## The concrete plan

This isn't a one-hour patch. But it's not a multi-week project either. The pieces:

| Piece | Effort | Impact |
|---|---|---|
| **A. Public tree data structure** | 1 day | Enables everything else |
| **B. Range vector propagation** (reach probs as `[f32; 1326]`) | 1 day | The core algorithmic change |
| **C. O(n) terminal eval via rank sorting + inclusion-exclusion** | 0.5 day | ~7× on top of A+B |
| **D. Suit isomorphism canonicalization** | 0.5 day | ~21× tree reduction |
| **E. Rayon parallel at flop chance nodes** | 0.25 day | 6× on 8 cores |
| **Total** | **~3 days** | **~500–1000× total** |

After A+B alone (the minimum viable), our 3-hour BR drops to ~15 min. Adding C+D+E gets it to ~2 min. That's the level where **20,000-deal BR becomes a 5-minute operation**, and exploitability becomes a real-time metric during training rather than a post-hoc diagnostic.

## Why the current memoization idea doesn't work

Just to close that loop: memoizing `(hash, bucket) → value` across deals is **semantically wrong** in sampled BR. Two deals with the same abstract hash have different concrete cards, hence different showdown values. The "solve once ever" language in the playbook refers to the CFV accumulation *within one iteration of policy improvement* — which is already what `cfv[hash][a] += value * prior` does. There's nothing to add there; the real win is restructuring the walk itself.

## Reference implementations to cross-read

- **OpenSpiel** `best_response.cc` — [DeepMind's reference](https://github.com/google-deepmind/open_spiel/blob/master/open_spiel/algorithms/best_response.cc), uses public tree via `HistoryTree`. The header comment notes *"A partially computed best-response can be computed when using a prob_cut_threshold >= 0"* — that's a **pruning knob** orthogonal to iteration count: skip subtrees where the opponent's reach probability is below threshold. For us, `prob_cut_threshold = 1e-4` would kill most of the deep tree without bias.
- **The 2024 CTU thesis** ["Pruning for Best-Response Algorithms"](https://dspace.cvut.cz/entities/publication/9dbab466-6537-483a-82fd-f983b9227f42) — has an OpenSpiel-based implementation with public-tree traversal plus pruning. It's Leduc-focused but the code structure is directly portable.
- **CFR-BR** (Johanson et al. AAAI 2012) — [paper](https://poker.cs.ualberta.ca/publications/AAAI12-cfrbr.pdf) — uses the accelerated BR walker as an inner loop of a *training* algorithm. If we ever want to compute optimal abstract strategies rather than just measure exploitability, this is the algorithm.

## Recommendation

Given where we are:

1. **Ship the current `PKR_BR_ITERATIONS` patch anyway** — it's committed, harmless, and gives us a stopgap for v16's evaluation.
2. **Set `EVAL_DEALS=500` for v16** and accept ±300 mbb precision for now.
3. **After v16**, implement pieces **A + B + C** (the minimum viable public-tree BR). That's ~2.5 days of work but turns exploitability into a **sub-minute operation at 5,000 deals**, which is the precision we actually need for A/B comparisons.

Not doing **D + E** initially is fine — they're incremental wins once A+B+C are in.

This is a proper engineering project, not a hack. The 3-day estimate assumes the Rust implementation goes smoothly — which, given how well the rest of the session has gone, seems reasonable.

Want me to start writing the public-tree BR module (pieces A and B) as a scaffold while v16 finishes?
