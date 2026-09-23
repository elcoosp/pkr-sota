#!/usr/bin/env bash
# verify_artifacts.sh — sanity-check a version dir before training.
# Catches the k=8-in-production incident class (r3 §17).
# Usage: scripts/verify_artifacts.sh outputs/v15
set -euo pipefail

D="${1:?usage: $0 <version_dir>}"
[ -d "$D" ] || { echo "FAIL: $D is not a directory" >&2; exit 1; }
fail() { echo "FAIL: $*" >&2; exit 1; }

# 1. no symlinks — every .bin must be a real file in this dir
for f in "$D"/*.bin; do
    [ -L "$f" ] && fail "$f is a symlink (cross-version contamination)"
done

size() { stat -f%z "$1" 2>/dev/null || stat -c%s "$1"; }

# 2. centroid floor (k=200 => ~1.6KB; k=8 was 72B)
[ -e "$D/centroids.bin" ] || fail "$D/centroids.bin missing"
cs=$(size "$D/centroids.bin")
[ "$cs" -lt 800 ] && fail "centroids.bin = ${cs}B - k=8 leak? (need >= 800)"

# 3. manifest (if it records centroid_k, enforce >= 100)
if [ -e "$D/manifest.txt" ] && grep -q "centroid_k=" "$D/manifest.txt"; then
    k=$(grep -oE "centroid_k=[0-9]+" "$D/manifest.txt" | head -1 | cut -d= -f2)
    [ "$k" -lt 100 ] && fail "manifest centroid_k=$k < 100"
fi

# 4. preflop table size
[ -e "$D/preflop_abstraction.bin" ] || fail "$D/preflop_abstraction.bin missing"
ps=$(size "$D/preflop_abstraction.bin")
[ "$ps" -ne 1326 ] && fail "preflop_abstraction.bin = ${ps}B, expected 1326"

# 5. required tables present
for f in hand_ranks.bin abstraction.bin turn_abstraction.bin \
         river_buckets.bin flop_buckets.bin; do
    [ -e "$D/$f" ] || fail "$D/$f missing"
done

echo "artifacts OK: $D (centroids=${cs}B)"
