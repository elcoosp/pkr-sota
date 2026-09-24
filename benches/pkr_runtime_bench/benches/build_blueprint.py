#!/usr/bin/env python3
"""Generate a synthetic blueprint.bin with N sorted u64 keys
and uniform-random CDF rows, for runtime lookup benchmarking
without paying the cost of training.

Usage:
    python3 build_blueprint.py <out_path> <num_keys> <max_actions_k>

IMPORTANT (worklog B7): the plan draft described the v2 layout
([FileHeader:32][key_count:4][cdf_size:4][keys][cdf] with FileHeader
fields magic/version/variant/count/k/hash_algo/pad). The real writer
(`crates/pkr-export/src/writer.rs`) emits FORMAT_VERSION_V4:

    [FileHeader:32][AnchorsSection:48][Fingerprint:40]
    [key_count:u32][cdf_size:u32][keys:u64 LE][cdf:u8]

FileHeader (bytemuck, LE):
    magic[8], version u32 (=4), variant_id u32 (=0),
    infoset_count u64, max_actions_k u8, hash_algo u8 (=2), pad[6]

AnchorsSection: 12 x f32 LE — row0 zeros, rows1-3 = [0.5, 1.0, 2.0]
(BET_SIZINGS in crates/pkr-core/src/abstraction.rs).

Fingerprint (40 B): preflop_k u32, flop_k u32 (=0), river_buckets u32 (=0),
sizing 3 x f32 (0.5, 1.0, 2.0), thresholds 2 x f32 (0.6, 1.2),
sig_version u8 (=1), hash_algo u8 (=2), river_tier_shift u8 (=13, T2.2),
pad[5].

MmapReader::new accepts v2/v3/v4 and does NOT validate fingerprint
contents at load, so this synthetic file loads cleanly. The runtime
lookup path (keys_data/cdf_data) is identical for synthetic and
trained blueprints.
"""
import struct
import sys
import random

MAGIC = b"PKRSOTA1"
FORMAT_VERSION_V4 = 4
HASH_ALGO_FNV1A64_INFOSET = 2


def main():
    out_path = sys.argv[1]
    n_keys = int(sys.argv[2])
    max_actions_k = int(sys.argv[3]) if len(sys.argv) > 3 else 8

    keys = sorted({random.getrandbits(64) for _ in range(n_keys * 2)})[:n_keys]
    while len(keys) < n_keys:
        keys = sorted(set(keys) | {random.getrandbits(64) for _ in range(n_keys)})
        keys = keys[:n_keys]
    cdf_size = n_keys * max_actions_k

    with open(out_path, "wb") as f:
        # FileHeader: 32 bytes — magic(8) + version(4) + variant(4) +
        # infoset_count(8) + max_actions_k(1) + hash_algo(1) + pad(6)
        f.write(MAGIC)
        f.write(struct.pack("<I", FORMAT_VERSION_V4))
        f.write(struct.pack("<I", 0))  # variant_id
        f.write(struct.pack("<Q", n_keys))  # infoset_count
        f.write(struct.pack("<B", max_actions_k))
        f.write(struct.pack("<B", HASH_ALGO_FNV1A64_INFOSET))
        f.write(b"\x00" * 6)  # pad
        # AnchorsSection: 48 bytes, 12 x f32 LE.
        anchors = [0.0, 0.0, 0.0] + [0.5, 1.0, 2.0] * 3
        for a in anchors:
            f.write(struct.pack("<f", a))
        # Fingerprint: 40 bytes.
        f.write(struct.pack("<I", 8))  # preflop_k (smoke k=8)
        f.write(struct.pack("<I", 0))  # flop_k (untracked)
        f.write(struct.pack("<I", 0))  # river_buckets (untracked)
        for v in (0.5, 1.0, 2.0):  # sizing_small/medium/large
            f.write(struct.pack("<f", v))
        for v in (0.6, 1.2):  # threshold_small/large
            f.write(struct.pack("<f", v))
        f.write(struct.pack("<B", 1))  # sig_version
        f.write(struct.pack("<B", HASH_ALGO_FNV1A64_INFOSET))
        f.write(struct.pack("<B", 13))  # river_tier_shift (RIVER_TIER_SHIFT, T2.2)
        f.write(b"\x00" * 5)  # pad
        # Section: key_count (u32) + cdf_size (u32)
        f.write(struct.pack("<I", n_keys))
        f.write(struct.pack("<I", cdf_size))
        # keys: u64 x n_keys, LE
        for k in keys:
            f.write(struct.pack("<Q", k))
        # cdf: u8 x cdf_size, monotonic per row ending at 255
        for _ in range(n_keys):
            row = sorted(random.sample(range(0, 256), max_actions_k))
            # normalize to a CDF ending at 255
            cdf = []
            acc = 0
            s = sum(row) or 1
            for v in row:
                acc += int(v * 255 / s)
                cdf.append(min(acc, 255))
            cdf[-1] = 255
            f.write(bytes(cdf))

    print(f"wrote {out_path}: {n_keys} keys, cdf_size={cdf_size}")


if __name__ == "__main__":
    main()
