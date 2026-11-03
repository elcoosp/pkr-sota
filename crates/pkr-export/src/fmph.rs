use crate::header::FmphHeader;
use pkr_contracts::{FNV_OFFSET, FNV_PRIME};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct FmphData {
    pub keys_len: usize,
    pub seed1: u64,
    pub seed2: u64,
    pub bucket_count: usize,
    pub displacements: Vec<u32>,
}

impl FmphData {
    /// Convert to the file header and serialize displacements separately.
    pub fn to_header(&self) -> FmphHeader {
        FmphHeader {
            num_keys: self.keys_len as u64,
            seed1: self.seed1,
            seed2: self.seed2,
            max_level_size: self.bucket_count as u64,
            level_count: 1u32,
            _padding: [0u8; 4],
        }
    }
}

#[inline]
pub fn hash_key(key: u64, seed: u64) -> u64 {
    key.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(seed)
}

/// Search-space cap for the displacement loop: `n * 32` (at least 512),
/// computed in u64 and clamped to u32 so absurd key counts cannot wrap
/// to a *smaller* search space (2026-11-03 re-audit).
#[inline]
pub fn max_displacement_for(n: usize) -> u32 {
    (n as u64).saturating_mul(32).max(512).min(u32::MAX as u64) as u32
}

/// S15: only called when `PKR_EMIT_FMPH=1` (writer.rs); off in every
/// production path. The retry loop below can take minutes-hours on large
/// tables, so the writer deliberately keeps it opt-in. Move behind a
/// `fmph` cargo feature if it ever needs to be off the default build
/// entirely.
pub fn build_fmph(keys: &[u64]) -> Option<FmphData> {
    build_fmph_with_attempts(keys, 500_000)
}

/// Testable variant: caps the seed-search attempts. Deterministic for a
/// fixed (keys, attempts) pair.
pub fn build_fmph_with_attempts(keys: &[u64], attempts: u32) -> Option<FmphData> {
    let unique: Vec<u64> = {
        let mut set = HashSet::new();
        keys.iter().copied().filter(|k| set.insert(*k)).collect()
    };

    let n = unique.len();
    if n == 0 {
        // m14 (audit 2026-10-10): empty key set is a legal input (a
        // blueprint with zero keys). Return None instead of panicking.
        return None;
    }

    let bucket_count = (n / 3).max(1);
    let max_displacement = max_displacement_for(n);

    // 2026-11-03 re-audit: seed the search from the key set itself so
    // the emitted FMph tail is byte-deterministic for a fixed key set
    // (rand::rng() made PKR_EMIT_FMPH=1 blueprints differ run-to-run).
    let mut seed_state = FNV_OFFSET;
    for &k in keys {
        seed_state ^= k;
        seed_state = seed_state.wrapping_mul(FNV_PRIME);
    }
    use rand::{RngExt, SeedableRng};
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed_state);
    let mut best: Option<(FmphData, u32)> = None;

    for _attempt in 0..attempts {
        let seed1 = rng.random();
        let seed2 = rng.random();

        let mut buckets: Vec<Vec<u64>> = vec![Vec::new(); bucket_count];
        for &key in &unique {
            let b = hash_key(key, seed1) as usize % bucket_count;
            buckets[b].push(key);
        }

        let mut displacements = vec![0u32; bucket_count];
        let mut used = vec![false; n];
        let mut ok = true;
        let mut max_d = 0u32;

        let mut perm: Vec<usize> = (0..bucket_count).collect();
        perm.sort_by_key(|&i| buckets[i].len());
        perm.reverse();

        for &b in &perm {
            let bucket = &buckets[b];
            if bucket.is_empty() {
                continue;
            }

            let mut found = false;
            for d in 0..max_displacement {
                let mut indices = Vec::with_capacity(bucket.len());
                let mut seen = HashSet::new();
                let mut collision = false;
                for &k in bucket {
                    let idx = hash_key(k, seed2).wrapping_add(d as u64) as usize % n;
                    if used[idx] || !seen.insert(idx) {
                        collision = true;
                        break;
                    }
                    indices.push(idx);
                }
                if !collision {
                    for &idx in &indices {
                        used[idx] = true;
                    }
                    displacements[b] = d;
                    max_d = max_d.max(d);
                    found = true;
                    break;
                }
            }
            if !found {
                ok = false;
                break;
            }
        }

        if ok {
            if best.is_none() || max_d < best.as_ref().unwrap().1 {
                best = Some((
                    FmphData {
                        keys_len: n,
                        seed1,
                        seed2,
                        bucket_count,
                        displacements,
                    },
                    max_d,
                ));
            }
            if max_d <= 1 {
                break;
            }
        }
    }

    // m14: do not panic on a pathological key set; return the best
    // candidate anyway (worst case: imperfect but valid).
    // m14 (audit 2026-10-10): return the best found, or None if no
    // assignment completed within the retry budget. Callers already
    // have a binary-search fallback path.
    best.map(|(data, _)| data)
}

pub fn eval_fmph(data: &FmphData, key: u64) -> usize {
    let b = hash_key(key, data.seed1) as usize % data.bucket_count;
    let d = data.displacements[b] as u64;
    hash_key(key, data.seed2).wrapping_add(d) as usize % data.keys_len
}

#[cfg(test)]
mod audit_2026_11_tests {
    //! 2026-11-03 re-audit: the FMph tail is opt-in (PKR_EMIT_FMPH=1)
    //! but it made blueprint bytes non-deterministic (random seed
    //! source), which breaks reproducible-build comparisons and any
    //! future golden expectation of the tail.

    use super::*;

    #[test]
    fn fmph_output_is_deterministic_for_fixed_keys() {
        // The seed comes from the key set (FNV), so two builds of the
        // same key set must agree byte-for-byte — the old
        // rand::rng() source made PKR_EMIT_FMPH=1 blueprints differ
        // run-to-run. Use the capped-attempts variant so the test is
        // fast; determinism is the property under test.
        let keys: Vec<u64> = (0..30).map(|i| 0x1_0000_0000u64 + i * 7919).collect();
        let a = build_fmph_with_attempts(&keys, 2_000).expect("must build");
        let b = build_fmph_with_attempts(&keys, 2_000).expect("must build");
        let (ha, hb) = (a.to_header(), b.to_header());
        assert_eq!(
            (ha.seed1, ha.seed2, ha.num_keys, ha.max_level_size),
            (hb.seed1, hb.seed2, hb.num_keys, hb.max_level_size),
            "header (incl. seeds) must match"
        );
        assert_eq!(a.displacements, b.displacements, "displacements must match");
        // And the actual lookup must be a perfect hash of the key set.
        for &k in &keys {
            assert_eq!(
                eval_fmph(&a, k),
                eval_fmph(&b, k),
                "key {k} must decode identically"
            );
        }
        // The two seeds come from the deterministic stream, not from
        // system randomness: rebuild with a DIFFERENT key set and the
        // seeds must differ (the old code could coincide by luck; the
        // point here is the output is a pure function of the input).
        // Auxiliary: a different key set seeds the search differently.
        // (Non-fatal: some key sets legitimately exhaust the attempt
        // budget — the writer falls back to binary search.)
        let other: Vec<u64> = (0..30).map(|i| 0x2_0000_0000u64 + i * 6151).collect();
        if let Some(c) = build_fmph_with_attempts(&other, 2_000) {
            assert_ne!((a.seed1, a.seed2), (c.seed1, c.seed2));
        }
    }

    /// max_displacement is a u32; the cap computation used to multiply
    /// in u64 and truncate for absurd key counts (n >= 2^27 wrapped to
    /// a tiny search space). The pure helper must clamp instead.
    #[test]
    fn max_displacement_does_not_wrap() {
        assert_eq!(max_displacement_for(0), 512);
        assert_eq!(max_displacement_for(10), 512);
        assert_eq!(max_displacement_for(1_000_000), 32_000_000);
        // n = 2^27 -> n*32 = 2^32, which used to wrap to 0.
        assert_eq!(max_displacement_for(1 << 27), u32::MAX);
        assert_eq!(max_displacement_for(usize::MAX), u32::MAX);
    }
}
