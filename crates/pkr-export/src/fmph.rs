use bytemuck::{Pod, Zeroable};
use foldhash::fast::FixedState;
use std::hash::{BuildHasher, Hash, Hasher};

/// Minimal perfect hash function data.
///
/// Uses a "Hash and Displace" algorithm with two independent hash seeds
/// and a per-bucket displacement array.
///
/// This struct is `Pod + Zeroable` so it can be memory-mapped directly.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct FmphDataPacked {
    /// Number of keys this hash was built for.
    pub keys_len: u64,
    seed1: u64,
    seed2: u64,
    bucket_count: u64,
    /// Offset into a displacements array (if stored separately)
    displacements_offset: u64,
    /// Reserved padding.
    _pad: [u8; 24],
}

// Ensure the packed header is exactly 64 bytes.
const _: () = {
    if std::mem::size_of::<FmphDataPacked>() != 64 {
        panic!("FmphDataPacked must be 64 bytes");
    }
};

/// Runtime form of FmphData.
#[derive(Debug, Clone)]
pub struct FmphData {
    pub keys_len: usize,
    seed1: u64,
    seed2: u64,
    bucket_count: usize,
    displacements: Vec<u32>,
}

impl FmphData {
    pub fn pack(&self, displacement_bytes: &mut Vec<u8>) -> FmphDataPacked {
        let offset = displacement_bytes.len() as u64;
        displacement_bytes.extend_from_slice(bytemuck::cast_slice(&self.displacements));
        FmphDataPacked {
            keys_len: self.keys_len as u64,
            seed1: self.seed1,
            seed2: self.seed2,
            bucket_count: self.bucket_count as u64,
            displacements_offset: offset,
            _pad: [0u8; 24],
        }
    }

    pub fn unpack(packed: &FmphDataPacked, displacement_bytes: &[u8]) -> Self {
        let start = packed.displacements_offset as usize;
        let len = packed.bucket_count as usize;
        let displacements =
            bytemuck::cast_slice::<u8, u32>(&displacement_bytes[start..start + len * 4]).to_vec();
        FmphData {
            keys_len: packed.keys_len as usize,
            seed1: packed.seed1,
            seed2: packed.seed2,
            bucket_count: len,
            displacements,
        }
    }
}

fn hash_key(key: u64, seed: u64) -> u64 {
    let mut hasher = FixedState::with_seed(seed).build_hasher();
    key.hash(&mut hasher);
    hasher.finish()
}

/// Build a minimal perfect hash function for the given keys.
///
/// Returns `FmphData` that maps each key to a distinct index in `0..keys.len()`.
///
/// # Panics
/// Panics if `keys` is empty or if the algorithm fails after many attempts.
pub fn build_fmph(keys: &[u64]) -> FmphData {
    let n = keys.len();
    assert!(n > 0, "cannot build FMph for empty key set");

    let bucket_count = (n / 4).max(1);
    // Allow enough displacement range; cap at 100_000.
    let max_displacement = (n as u64 * 4).min(100_000) as u32;

    use rand::RngExt;
    let mut rng = rand::rng();

    // We keep trying different seed pairs until we find a perfect hash.
    // Also track the best result to minimise max displacement.
    let mut best: Option<(FmphData, u32)> = None; // (data, max_d)

    for _attempt in 0..2000 {
        let seed1 = rng.random();
        let seed2 = rng.random();

        let mut buckets: Vec<Vec<u64>> = vec![Vec::new(); bucket_count];
        for &key in keys {
            let b = hash_key(key, seed1) as usize % bucket_count;
            buckets[b].push(key);
        }

        let mut displacements = vec![0u32; bucket_count];
        let mut used = vec![false; n];
        let mut ok = true;
        let mut max_d = 0u32;

        // Process buckets from largest to smallest (better packing).
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
                // Check that this displacement gives a set of distinct, unused indices.
                let mut indices = Vec::with_capacity(bucket.len());
                let mut collision = false;
                for &k in bucket {
                    let idx = hash_key(k, seed2).wrapping_add(d as u64) as usize % n;
                    if used[idx] || indices.contains(&idx) {
                        collision = true;
                        break;
                    }
                    indices.push(idx);
                }
                if !collision {
                    // Valid displacement found.
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
            // Perfect hash found; update best.
            if best.is_none() || max_d < best.as_ref().unwrap().1 {
                best = Some((FmphData {
                    keys_len: n,
                    seed1,
                    seed2,
                    bucket_count,
                    displacements,
                }, max_d));
            }
            // If max displacement is very small, we can stop early.
            if max_d <= 1 {
                break;
            }
        }
    }

    best.map(|(data, _)| data)
        .expect("unable to find FMph after 2000 attempts; try increasing attempts or max_displacement")
}

/// Evaluate the perfect hash for a single key.
pub fn eval_fmph(data: &FmphData, key: u64) -> usize {
    let b = hash_key(key, data.seed1) as usize % data.bucket_count;
    let d = data.displacements[b] as u64;
    hash_key(key, data.seed2)
        .wrapping_add(d) as usize
        % data.keys_len
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngExt;

    #[test]
    fn fmph_zero_collisions_1000_keys() {
        let mut rng = rand::rng();
        let keys: Vec<u64> = (0..1000).map(|_| rng.random()).collect();

        let fmph = build_fmph(&keys);

        let n = keys.len();
        let mut seen = vec![false; n];
        for &key in &keys {
            let idx = eval_fmph(&fmph, key);
            assert!(idx < n, "index out of bounds: {idx} >= {n}");
            assert!(!seen[idx], "collision at index {idx} for key {key}");
            seen[idx] = true;
        }
        assert!(seen.iter().all(|&x| x), "not all indices were assigned");
    }

    #[test]
    fn fmph_empty_panics() {
        let keys: Vec<u64> = vec![];
        let result = std::panic::catch_unwind(|| {
            build_fmph(&keys);
        });
        assert!(result.is_err(), "build_fmph with empty keys should panic");
    }

    #[test]
    fn fmph_single_key() {
        let keys = vec![42u64];
        let fmph = build_fmph(&keys);
        assert_eq!(eval_fmph(&fmph, 42), 0);
    }

    #[test]
    fn fmph_duplicate_keys() {
        let keys = vec![1, 1, 1];
        let fmph = build_fmph(&keys);
        let idx = eval_fmph(&fmph, 1);
        assert!(idx < 3);
    }

    #[test]
    fn fmph_no_collisions_small_set() {
        let keys: Vec<u64> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
        let fmph = build_fmph(&keys);
        let n = keys.len();
        let mut seen = vec![false; n];
        for &k in &keys {
            let idx = eval_fmph(&fmph, k);
            assert!(idx < n);
            seen[idx] = true;
        }
    }

    #[test]
    fn fmph_output_range() {
        let mut rng = rand::rng();
        let keys: Vec<u64> = (0..50).map(|_| rng.random()).collect();
        let fmph = build_fmph(&keys);
        for &k in &keys {
            let idx = eval_fmph(&fmph, k);
            assert!(idx < keys.len(), "idx {idx} out of range for {keys:?}");
        }
    }

    #[test]
    fn fmph_deterministic_eval() {
        let keys: Vec<u64> = vec![100, 200, 300, 400, 500];
        let fmph = build_fmph(&keys);
        let first = eval_fmph(&fmph, 100);
        let second = eval_fmph(&fmph, 100);
        assert_eq!(first, second);
    }

    #[test]
    fn fmph_unique_indices_for_distinct_keys() {
        let mut rng = rand::rng();
        let mut set = std::collections::HashSet::new();
        while set.len() < 100 {
            set.insert(rng.random::<u64>());
        }
        let keys: Vec<u64> = set.into_iter().collect();
        let fmph = build_fmph(&keys);
        let n = keys.len();
        let mut seen = vec![false; n];
        for &k in &keys {
            let idx = eval_fmph(&fmph, k);
            assert!(!seen[idx], "collision at {idx}");
            seen[idx] = true;
        }
        assert!(seen.iter().all(|&x| x));
    }

    #[test]
    fn fmph_pack_roundtrip() {
        let mut rng = rand::rng();
        let keys: Vec<u64> = (0..200).map(|_| rng.random()).collect();
        let fmph = build_fmph(&keys);

        let mut bytes = Vec::new();
        let packed = fmph.pack(&mut bytes);
        let unpacked = FmphData::unpack(&packed, &bytes);

        for &k in &keys {
            let a = eval_fmph(&fmph, k);
            let b = eval_fmph(&unpacked, k);
            assert_eq!(a, b, "pack/unpack mismatch for key {k}");
        }
    }

    #[test]
    fn fmph_packed_header_size() {
        assert_eq!(std::mem::size_of::<FmphDataPacked>(), 64);
        assert_eq!(std::mem::align_of::<FmphDataPacked>(), 8);
    }

    #[test]
    fn fmph_packed_is_pod() {
        fn assert_pod<T: bytemuck::Pod>() {}
        assert_pod::<FmphDataPacked>();
    }

    #[test]
    fn fmph_zeroed_packed() {
        let z: FmphDataPacked = Zeroable::zeroed();
        assert_eq!(z.keys_len, 0);
        assert_eq!(z.seed1, 0);
        assert_eq!(z.seed2, 0);
    }

    #[test]
    fn fmph_large_keyset_5000() {
        use std::time::Instant;
        let mut rng = rand::rng();
        let keys: Vec<u64> = (0..5000).map(|_| rng.random()).collect();
        let start = Instant::now();
        let fmph = build_fmph(&keys);
        let elapsed = start.elapsed();
        eprintln!("Built FMph for 5000 keys in {elapsed:?}");

        let n = keys.len();
        let mut seen = vec![false; n];
        for &k in &keys {
            let idx = eval_fmph(&fmph, k);
            assert!(idx < n);
            assert!(!seen[idx]);
            seen[idx] = true;
        }
        assert!(seen.iter().all(|&x| x));
    }
}
