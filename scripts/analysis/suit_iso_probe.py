#!/usr/bin/env python3
"""
Suit-isomorphism probe for the river bucket precompute.

For each 5-card board (52 choose 5), EHS depends on:
  - the rank multiset (e.g. {A,K,Q,7,2})
  - the suit equivalence pattern (which cards share a suit)
  - the flush-suit membership (at most one suit has 5+)
Not on which specific suits appear.

Under the symmetric group S_4 acting on suit labels, many boards are
equivalent. If we can canonicalize each board to a unique representative
of its S_4 orbit, we compute EHS once per orbit and copy the result.

This script:
  1. Samples N distinct boards
  2. Canonicalizes each by applying the S_4 permutation that
     lexicographically minimizes the board's "suit signature"
  3. Counts distinct canonical forms
  4. Reports the dedup factor

The dedup factor tells us how much faster a Rust port would be.
"""
import itertools
import random
import sys

# Card = suit * 13 + rank, suit in 0..3, rank in 0..12
def suit(c): return c // 13
def rank(c): return c % 13

# Given 5 cards (tuple), return canonical form under S_4 relabeling.
# Approach: try all 24 permutations of {0,1,2,3}; for each, relabel
# every card's suit; keep the lexicographically smallest sorted tuple.
def canonicalize(board):
    best = None
    for perm in itertools.permutations((0, 1, 2, 3)):
        relabeled = tuple(sorted(perm[suit(c)] * 13 + rank(c) for c in board))
        if best is None or relabeled < best:
            best = relabeled
    return best

def main(n_samples=50_000, seed=42):
    rng = random.Random(seed)
    # Take a random distinct sample (not exhaustive; C(52,5)=2.6M is
    # fine in Python but slow for a probe; 50K is representative).
    seen = set()
    canonicals = {}
    dups = 0
    for _ in range(n_samples):
        while True:
            board = tuple(sorted(rng.sample(range(52), 5)))
            if board not in seen:
                seen.add(board)
                break
        c = canonicalize(board)
        if c in canonicals:
            dups += 1
        canonicals[c] = board

    print(f"sampled boards:      {len(seen)}")
    print(f"distinct canonicals: {len(canonicals)}")
    print(f"dups collapsed:      {dups} ({100.0*dups/len(seen):.1f}%)")
    print(f"effective dedup:     {len(seen)/len(canonicals):.2f}x")
    print()

    # Histogram of orbit sizes observed
    from collections import Counter
    orbit_sizes = Counter()
    for b in seen:
        c = canonicalize(b)
        orbit_sizes[c] += 1
    size_hist = Counter(orbit_sizes.values())
    print("orbit-size histogram (top 10):")
    for size, count in size_hist.most_common(10):
        print(f"  size {size:>3}: {count:>6} orbits")
    print()

    # Sanity: canonicalization must preserve rank multiset
    for _ in range(200):
        b = tuple(sorted(rng.sample(range(52), 5)))
        c = canonicalize(b)
        assert sorted(rank(x) for x in b) == sorted(rank(x) for x in c), \
            f"rank multiset changed: {b} -> {c}"
        # Suit-count multiset preserved (which cards share suits)
        def suit_counts(bd):
            counts = {}
            for x in bd:
                counts[suit(x)] = counts.get(suit(x), 0) + 1
            return tuple(sorted(counts.values()))
        assert suit_counts(b) == suit_counts(c), \
            f"suit pattern changed: {b} -> {c}"
    print("sanity check: canonicalization preserves rank multiset and suit pattern [OK]")

if __name__ == "__main__":
    main()
