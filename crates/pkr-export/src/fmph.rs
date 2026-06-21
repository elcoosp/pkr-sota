use foldhash::fast::FixedState;
use std::hash::{BuildHasher, Hash, Hasher};

/// Minimal perfect hash function data.
///
/// Uses a "Hash and Displace" algorithm with two independent hash seeds
/// and a per-bucket displacement array.
#[derive(Debug, Clone)]
pub struct FmphData {
    /// Number of keys this hash was built for.
    pub keys_len: usize,
    seed1: u64,
    seed2: u64,
    bucket_count: usize,
    displacements: Vec<u32>,
}

/// Hash a single `u64` key with a given seed, producing a `u64` digest.
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
/// Panics if `keys` is empty (division by zero would occur).
pub fn build_fmph(keys: &[u64]) -> FmphData {
    let n = keys.len();
    assert!(n > 0, "cannot build FMph for empty key set");

    // Roughly 4 keys per bucket on average — gives fast displacement search.
    let bucket_count = (n / 4).max(1);
    let max_displacement = 10_000;

    use rand::RngExt;
    let mut rng = rand::rng();

    loop {
        let seed1 = rng.random();
        let seed2 = rng.random();

        // Partition keys into buckets using the first hash.
        let mut buckets: Vec<Vec<u64>> = vec![Vec::new(); bucket_count];
        for &key in keys {
            let b = hash_key(key, seed1) as usize % bucket_count;
            buckets[b].push(key);
        }

        let mut displacements = vec![0u32; bucket_count];
        let mut used = vec![false; n];
        let mut success = true;

        for b in 0..bucket_count {
            let bucket = &buckets[b];
            if bucket.is_empty() {
                continue;
            }

            let mut found = false;
            for d in 0..max_displacement {
                // Check whether displacement d works for this bucket.
                let ok = bucket.iter().all(|&k| {
                    let idx = hash_key(k, seed2).wrapping_add(d as u64) as usize % n;
                    !used[idx]
                });

                if ok {
                    // Mark the indices as used.
                    for &k in bucket {
                        let idx = hash_key(k, seed2).wrapping_add(d as u64) as usize % n;
                        used[idx] = true;
                    }
                    displacements[b] = d;
                    found = true;
                    break;
                }
            }

            if !found {
                success = false;
                break;
            }
        }

        if success {
            return FmphData {
                keys_len: n,
                seed1,
                seed2,
                bucket_count,
                displacements,
            };
        }
        // Try again with fresh seeds.
    }
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
            assert!(
                idx < n,
                "index out of bounds: {} >= {}",
                idx,
                n
            );
            assert!(
                !seen[idx],
                "collision at index {} for key {}",
                idx,
                key
            );
            seen[idx] = true;
        }
        assert!(
            seen.iter().all(|&x| x),
            "not all indices were assigned"
        );
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
}
