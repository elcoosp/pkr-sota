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
    [ -e "$f" ] || continue
    [ -L "$f" ] && fail "$f is a symlink (cross-version contamination)"
done

# 2026-11-03 re-audit (M2): `stat -f%z` is BSD-only and GNU `stat -f`
# means "filesystem mode", so the old fallback could misfire. `wc -c` is
# portable across mac/linux.
size() { wc -c < "$1" | tr -d '[:space:]'; }

# 2026-11-03 re-audit (M2): presence checks alone let a truncated
# turn/flop/river table be cached forever by run.sh's `[ -s "$target" ]`.
# Assert exact byte sizes for every artifact run.sh generates. Entry
# counts are derived from crates/pkr-abstraction/src/bin/precompute.rs:
#   hand_ranks.bin          = C(52,5) u32                  = 10395840
#   preflop_abstraction.bin = 1326 x u8 (always u8 width)  =    1326
#   abstraction.bin (flop)  = C(52,5) x 10 bucket entries  = 25989600
#   turn_abstraction.bin    = C(52,6) x 15 bucket entries  = 305377800
#   river_buckets.bin       = C(52,5) bucket entries       =  2598960
#   flop_buckets.bin        = C(52,3) bucket entries       =    22100
# Bucket tables are 1 byte/entry by default and 2 bytes/entry under
# PKR_BUCKET_U16=1 (precompute's buckets_to_bytes); accept either
# width, defaulting to run-config.sh's u8 configuration.
W=1
if [ "${PKR_BUCKET_U16:-0}" = "1" ]; then W=2; fi

check_exact() { # name expected
    local f="$D/$1" exp="$2" got
    [ -e "$f" ] || fail "$D/$1 missing"
    got=$(size "$f")
    [ "$got" -ne "$exp" ] && fail "$D/$1 = ${got}B, expected $2"
    echo "  ok: $1 = ${got}B"
}

# 2. centroid floor (k=200 => ~1.6KB; k=8 was 72B). Variable size, so a
#    floor is all we can assert.
[ -e "$D/centroids.bin" ] || fail "$D/centroids.bin missing"
cs=$(size "$D/centroids.bin")
[ "$cs" -lt 800 ] && fail "centroids.bin = ${cs}B - k=8 leak? (need >= 800)"
if [ -e "$D/centroids_6d.bin" ]; then
    c6=$(size "$D/centroids_6d.bin")
    [ "$c6" -lt 800 ] && fail "centroids_6d.bin = ${c6}B - k=8 leak? (need >= 800)"
fi

# 3. manifest (if it records centroid_k, enforce >= 100)
if [ -e "$D/manifest.txt" ] && grep -q "centroid_k=" "$D/manifest.txt"; then
    k=$(grep -oE "centroid_k=[0-9]+" "$D/manifest.txt" | head -1 | cut -d= -f2)
    [ "$k" -lt 100 ] && fail "manifest centroid_k=$k < 100"
fi

# 4. exact-size table (M2)
check_exact hand_ranks.bin          $((10395840))
check_exact preflop_abstraction.bin $((1326))
check_exact abstraction.bin         $((25989600 * W))
check_exact turn_abstraction.bin    $((305377800 * W))
check_exact river_buckets.bin       $((2598960 * W))
check_exact flop_buckets.bin        $((22100 * W))
# The smoke layout names the flop table flop_abstraction.bin; run.sh's
# version dirs use abstraction.bin. Enforce the same size when present.
if [ -e "$D/flop_abstraction.bin" ]; then
    fsz=$(size "$D/flop_abstraction.bin")
    [ "$fsz" -ne $((25989600 * W)) ] && \
        fail "flop_abstraction.bin = ${fsz}B, expected $((25989600 * W))"
fi

echo "artifacts OK: $D (centroids=${cs}B, bucket_width=${W}B)"
