#!/usr/bin/env bash
# bucket-membership.sh — for each hand class, show which bucket it's in
# and what other classes share that bucket.
set -uo pipefail
cd "$(dirname "$0")"

python3 - << 'PY'
from math import comb

def flat_index_preflop(c0, c1):
    a, b = sorted([c0, c1], reverse=True)
    return comb(a, 2) + comb(b, 1)

def to_cards(rank, suit):
    # rank 0..12 = 2..A, suit 0..3 = c,d,h,s
    return rank * 4 + suit

RANK_NAMES = "23456789TJQKA"

# Build all 169 hand classes with a representative combo each.
def hand_class(r1, r2, suited):
    # r1 >= r2, sorted descending rank
    if r1 == r2:
        return f"{RANK_NAMES[r1]}{RANK_NAMES[r2]}"
    suffix = "s" if suited else "o"
    return f"{RANK_NAMES[r1]}{RANK_NAMES[r2]}{suffix}"

# For each class, pick a canonical combo
classes = []
# Pairs: 13
for r in range(12, -1, -1):
    c0 = to_cards(r, 0)
    c1 = to_cards(r, 1)
    classes.append((hand_class(r, r, False), (c0, c1)))
# Suited and offsuit
for r1 in range(12, -1, -1):
    for r2 in range(r1 - 1, -1, -1):
        # suited: same suit
        classes.append((hand_class(r1, r2, True),
                        (to_cards(r1, 0), to_cards(r2, 0))))
        # offsuit: different suits
        classes.append((hand_class(r1, r2, False),
                        (to_cards(r1, 0), to_cards(r2, 1))))

# Read preflop table
tables = {
    'v9  (k=8)':  'outputs/v9/preflop_abstraction.bin',
    'v14 (k=200)': 'outputs/v14/preflop_abstraction.bin',
}

# For each table, group classes by bucket
for label, path in tables.items():
    try:
        data = open(path, 'rb').read()
    except FileNotFoundError:
        print(f"=== {label}: MISSING {path}")
        continue

    by_bucket = {}
    for cls, (c0, c1) in classes:
        idx = flat_index_preflop(c0, c1)
        b = data[idx] if idx < len(data) else None
        by_bucket.setdefault(b, []).append(cls)

    print("")
    print(f"=== {label}  ({len(by_bucket)} distinct buckets) ===")

    # Highlight the buckets for key hands
    key_hands = ['AA', 'KK', 'QQ', 'JJ', 'TT', '99', 'AKs', 'AQo', 'KQo', 'T9s', '72o', '22']
    print("")
    print("  Key hands and their bucket (with bucket size):")
    for h in key_hands:
        for cls, (c0, c1) in classes:
            if cls == h:
                idx = flat_index_preflop(c0, c1)
                b = data[idx]
                members = by_bucket[b]
                size = len(members)
                # Show up to 12 members to keep lines readable
                shown = members[:12]
                more = "" if size <= 12 else f" ... +{size-12} more"
                print(f"    {h:5s} -> bucket {b:3d} (size {size:3d}): {' '.join(shown)}{more}")
                break

    # Show the biggest buckets to spot check
    print("")
    print("  Largest 5 buckets:")
    for b, members in sorted(by_bucket.items(), key=lambda kv: -len(kv[1]))[:5]:
        shown = members[:16]
        more = "" if len(members) <= 16 else f" ... +{len(members)-16} more"
        print(f"    bucket {b:3d} (size {len(members):3d}): {' '.join(shown)}{more}")

    # Smallest buckets (all singletons?)
    singletons = [members[0] for members in by_bucket.values() if len(members) == 1]
    if singletons:
        print("")
        print(f"  Singleton buckets ({len(singletons)}):")
        # Show just the first 20
        print(f"    {' '.join(singletons[:20])}")
        if len(singletons) > 20:
            print(f"    ... +{len(singletons)-20} more")
PY

echo ""
echo "=== v14 current status ==="
pgrep -fl pkr-trainer || echo "  no trainer running"
tail -3 /tmp/v14_evals.log 2>/dev/null
