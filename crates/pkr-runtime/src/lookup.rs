use pkr_contracts::{BlueprintProvider, SotaAdvice};
use crate::mmap::MmapReader;

/// Fast-path runtime lookup engine.
///
/// Wraps the memory‑mapped blueprint file and performs an O(1) minimal‑perfect‑hash
/// lookup to retrieve the strategy CDF for a given information‑set hash.
pub struct SolverHandle {
    mmap: MmapReader,
}

impl SolverHandle {
    /// Constructs a new solver handle from an already validated `MmapReader`.
    pub fn new(mmap: MmapReader) -> Self {
        SolverHandle { mmap }
    }

    /// Returns the strategy advice for `infoset_hash`, if the hash maps to a valid infoset.
    pub fn get_advice_fast(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        let fh = self.mmap.file_header();
        if fh.infoset_count == 0 {
            return None;
        }

        // Evaluate the minimal perfect hash to obtain the infoset index.
        let idx = eval_mph(
            infoset_hash,
            fh.infoset_count as usize,
            self.mmap.fmph_header(),
            self.mmap.fmph_data(),
        );

        let max_actions = fh.max_actions_k as usize;
        let cdf_start = idx * max_actions;
        let cdf_end = cdf_start + max_actions;
        let cdf_slice = self.mmap.cdf_data();

        // Boundary check – should always pass with a well‑formed file.
        if cdf_end > cdf_slice.len() {
            return None;
        }

        Some(SotaAdvice {
            cdf_probabilities: cdf_slice[cdf_start..cdf_end].to_vec(),
        })
    }
}

impl BlueprintProvider for SolverHandle {
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        self.get_advice_fast(infoset_hash)
    }
}

// ---------------------------------------------------------------------------
// Internal minimal‑perfect‑hash evaluation (see test module for the builder)
// ---------------------------------------------------------------------------

#[inline]
fn hash_key(key: u64, seed: u64) -> u64 {
    // A cheap, branchless mixing function – fast and sufficient for MPH.
    key.wrapping_mul(0x9E3779B97F4A7C15) ^ seed
}

/// Evaluates the multi‑level displacement‑based MPH stored in the file.
fn eval_mph(
    key: u64,
    num_keys: usize,
    fmph_header: &pkr_export::header::FmphHeader,
    fmph_data: &[u8],
) -> usize {
    let level_count = fmph_header.level_count as usize;
    let max_level_size = fmph_header.max_level_size as usize;
    let seed = fmph_header.seed;

    let mut acc: u64 = 0;
    for l in 0..level_count {
        // Derive a level‑specific seed so that each level uses independent hashing.
        let level_seed = seed.wrapping_add(l as u64 * 0x9E3779B97F4A7C15);
        let h = hash_key(key, level_seed);
        let bucket = (h % max_level_size as u64) as usize;
        // Each level is stored as contiguous u32 values.
        let offset = (l * max_level_size + bucket) * 4;
        let d = u32::from_le_bytes(fmph_data[offset..offset + 4].try_into().unwrap());
        acc = acc.wrapping_add(d as u64);
    }
    (acc % num_keys as u64) as usize
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
use std::collections::HashSet;
mod tests {
    use super::*;
    use pkr_export::header::{FileHeader, FmphHeader, TranslationTableHeader};
    use std::io::Write;

    /// A self‑contained MPH builder for testing.
    ///
    /// Given distinct keys, constructs a single‑level displacement table that
    /// yields a perfect hash into `0..keys.len()`. Returns the `FmphHeader` and
    /// the raw displacement bytes.
    fn build_test_mph(keys: &[u64]) -> (FmphHeader, Vec<u8>) {
        assert!(!keys.is_empty(), "must have at least one key");
        let n = keys.len();
        let max_level_size = (n * 2).max(1); // generous bucket count
        let level_count = 1u32;
        let mut rng = simple_rng(42);

        for _attempt in 0..10_000 {
            let seed = rng.next_u64();
            let mut displacements = vec![0u32; max_level_size];
            let mut used = vec![false; n];
            let mut ok = true;
            for (bucket, bucket_keys) in keys.iter().map(|&k| {
                let h = hash_key(k, seed) as usize % max_level_size;
                (b, k)
            }).fold(vec![Vec::new(); max_level_size], |mut acc, (b, k)| {
                acc[b].push(k);
                acc
            }).into_iter().enumerate().filter(|(_: |(_, v)| !v.is_empty()usize, v: |(_, v)| !v.is_empty()Vec<u64>)| !v.is_empty()) {
                // For each bucket, try to find a displacement d such that all
                // keys in the bucket map to distinct unused slots.
                let mut found = false;
                for d in 0..(n as u32 * 4) {
                    let mut collision = false;
                    let mut slots = vec![];
                    for &k in &bucket_keys {
                        let idx = (hash_key(k, seed).wrapping_add(d as u64) % n as u64) as usize;
                        if used[idx] || slots.contains(&idx) {
                            collision = true;
                            break;
                        }
                        slots.push(idx);
                    }
                    if !collision {
                        for &idx in &slots {
                            used[idx] = true;
                        }
                        displacements[bucket] = d;
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
                let hdr = FmphHeader {
                    num_keys: n as u64,
                    seed,
                    max_level_size: max_level_size as u64,
                    level_count,
                    _padding: [0; 4],
                };
                let data = bytemuck::cast_slice::<u32, u8>(&displacements).to_vec();
                return (hdr, data);
            }
        }
        panic!("failed to build test MPH after many attempts");
    }

    /// Tiny deterministic RNG for reproducible tests.
    struct SimpleRng(u64);
    fn simple_rng(seed: u64) -> SimpleRng {
        SimpleRng(seed)
    }
    impl SimpleRng {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0
        }
    }

    /// Helper to write a complete blueprint file to `path` from the given pieces.
    fn write_test_blueprint(
        path: &str,
        file_header: &FileHeader,
        fmph_header: &FmphHeader,
        fmph_data: &[u8],
        cdf: &[u8],
    ) {
        let tt_header = TranslationTableHeader {
            num_entries: 0,
            action_size: 0,
            _padding: [0; 4],
        };
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(bytemuck::bytes_of(file_header)).unwrap();
        f.write_all(bytemuck::bytes_of(fmph_header)).unwrap();
        f.write_all(fmph_data).unwrap();
        f.write_all(bytemuck::bytes_of(&tt_header)).unwrap();
        f.write_all(cdf).unwrap();
        f.flush().unwrap();
    }

    // -----------------------------------------------------------------------
    // Original tests
    // -----------------------------------------------------------------------

    #[test]
    fn basic_lookup_returns_correct_cdf() {
        let keys: Vec<u64> = vec![100, 200, 300];
        let n = keys.len();
        let max_actions = 3u8;
        let (fmph_hdr, fmph_bytes) = build_test_mph(&keys);
        let cdf_bytes: Vec<u8> = (0..(n * max_actions as usize))
            .map(|i| (i + 1) as u8)
            .collect();

        let file_hdr = FileHeader {
            magic: *b"PKRSOTA1",
            version: 1,
            variant_id: 0,
            infoset_count: n as u64,
            max_actions_k: max_actions,
            _padding: [0; 7],
        };

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        write_test_blueprint(&path, &file_hdr, &fmph_hdr, &fmph_bytes, &cdf_bytes);

        let mmap = MmapReader::new(&path).unwrap();
        let solver = SolverHandle::new(mmap);

        for (expected_idx, &key) in keys.iter().enumerate() {
            let advice = solver.get_advice_fast(key).expect("key must be found");
            let start = expected_idx * max_actions as usize;
            let expected_slice = &cdf_bytes[start..start + max_actions as usize];
            assert_eq!(advice.cdf_probabilities, expected_slice,
                "mismatch for key {key} (expected idx {expected_idx})");
        }

        let unknown = solver.get_advice_fast(9999);
        let _ = unknown;
    }

    #[test]
    fn empty_blueprint_returns_none() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        let fh = FileHeader {
            magic: *b"PKRSOTA1",
            version: 1,
            variant_id: 0,
            infoset_count: 0,
            max_actions_k: 0,
            _padding: [0; 7],
        };
        let fmp_hdr = FmphHeader {
            num_keys: 0,
            seed: 0,
            max_level_size: 1,
            level_count: 1,
            _padding: [0; 4],
        };
        let tt_hdr = TranslationTableHeader {
            num_entries: 0,
            action_size: 0,
            _padding: [0; 4],
        };
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(bytemuck::bytes_of(&fh)).unwrap();
        f.write_all(bytemuck::bytes_of(&fmp_hdr)).unwrap();
        f.write_all(bytemuck::bytes_of(&tt_hdr)).unwrap();
        f.flush().unwrap();

        let mmap = MmapReader::new(&path).unwrap();
        let solver = SolverHandle::new(mmap);
        assert!(solver.get_advice_fast(42).is_none());
    }

    #[test]
    fn implements_blueprint_provider() {
        let keys = vec![7u64, 8, 9];
        let (fmph_hdr, fmph_bytes) = build_test_mph(&keys);
        let max_actions = 2u8;
        let cdf = vec![10u8, 20, 30, 40, 50, 60];
        let fh = FileHeader {
            magic: *b"PKRSOTA1",
            version: 1,
            variant_id: 0,
            infoset_count: 3,
            max_actions_k: max_actions,
            _padding: [0; 7],
        };
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        write_test_blueprint(&path, &fh, &fmph_hdr, &fmph_bytes, &cdf);
        let mmap = MmapReader::new(&path).unwrap();
        let solver = SolverHandle::new(mmap);
        let advice = solver.lookup(8).unwrap();
        let valid_slices: Vec<&[u8]> = cdf.chunks(2).collect();
        assert!(valid_slices.contains(&advice.cdf_probabilities.as_slice()));
    }

    // -----------------------------------------------------------------------
    // Additional tests for comprehensive coverage
    // -----------------------------------------------------------------------

    #[test]
    fn deterministic_output_for_same_key() {
        let keys = vec![42, 99, 123];
        let (fmph_hdr, fmph_bytes) = build_test_mph(&keys);
        let max_actions = 2;
        let cdf: Vec<u8> = (0..6).map(|i| (i * 40) as u8).collect();
        let fh = FileHeader {
            magic: *b"PKRSOTA1",
            version: 1,
            variant_id: 0,
            infoset_count: 3,
            max_actions_k: max_actions,
            _padding: [0; 7],
        };
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        write_test_blueprint(&path, &fh, &fmph_hdr, &fmph_bytes, &cdf);

        let mmap = MmapReader::new(&path).unwrap();
        let solver = SolverHandle::new(mmap);
        let first = solver.get_advice_fast(42).unwrap();
        let second = solver.get_advice_fast(42).unwrap();
        assert_eq!(first.cdf_probabilities, second.cdf_probabilities);
    }

    #[test]
    fn index_out_of_bounds_returns_none() {
        // Create a blueprint with infoset_count=1, max_actions=1, but corrupt
        // the MPH so that eval_mph returns index >= infoset_count.
        // We'll directly craft a file with a known MPH that maps a key to index 5
        // while infoset_count=1.
// //         let keys = vec![1u64];
// //         // Build a valid MPH for this single key (index 0)
// //         let (mut fmp_hdr, fmph_bytes) = build_test_mph(&keys);
//         // Override infoset_count to 1 but make MPH point to index 5 by manipulating
//         // seed and displacement so that eval_mph returns 5. We can modify the FmphHeader
//         // and data to cause eval_mph to return 5. Since our eval_mph uses seed and
//         // level_count=1, we can craft a displacement d such that for the given key,
//         // hash % n + d ≡ 5 mod 1? No, mod 1 always 0. So we need infoset_count=1,
//         // but if mph returns idx=5, cdf_start=5*1=5, cdf_end=6 > cdf_len=1, so should
//         // return None. To force idx=5, we can set infoset_count=6 but only provide 1
//         // byte of CDF, so boundary check fails. That's simpler: set infoset_count=6,
//         // max_actions=1, but CDF len=1 (only one byte). Then idx=0..5, only idx=0 is
//         // valid. We need a mph that maps a key to index 0? We want idx>=1 to trigger
//         // out-of-bounds. We'll create MPH for 6 keys, but cdf len = 1. Then any
//         // lookup that maps to idx>=1 will fail.
//         let keys_many: Vec<u64> = (0..6).map(|i| i as u64).collect();
        let (fmp_hdr_many, fmph_bytes_many) = build_test_mph(&keys_many);
        let fh = FileHeader {
            magic: *b"PKRSOTA1",
            version: 1,
            variant_id: 0,
            infoset_count: 6,
            max_actions_k: 1,
            _padding: [0; 7],
        };
        let cdf_short = vec![99u8]; // only 1 byte
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        write_test_blueprint(&path, &fh, &fmp_hdr_many, &fmph_bytes_many, &cdf_short);

        let mmap = MmapReader::new(&path).unwrap();
        let solver = SolverHandle::new(mmap);

        // Keys 0,1,2,3,4,5 are all in mph; but only the one whose idx=0 will succeed.
        // We'll test each and count successes. At most one should succeed if mph maps exactly
        // one to index 0. Actually, mph maps distinct keys to distinct indices 0..5.
        // So exactly one key will map to 0. Others should return None.
        let mut success_count = 0;
        for k in 0..6u64 {
            if solver.get_advice_fast(k).is_some() {
                success_count += 1;
            }
        }
        assert_eq!(success_count, 1, "exactly one key should map to index 0 and return Some");
    }

    #[test]
    fn large_keyset_stress_test() {
        // Generate 2000 random keys, build mph, assign CDF, verify no crashes and each key gets
        // a CDF slice of correct length.
        use std::collections::HashSet;
        let mut rng = simple_rng(999);
        let mut keys = HashSet::new();
        while keys.len() < 2000 {
            keys.insert(rng.next_u64());
        }
        let keys_vec: Vec<u64> = keys.into_iter().collect();
        let n = keys_vec.len();
        let max_actions = 4u8;
        let (fmph_hdr, fmph_bytes) = build_test_mph(&keys_vec);
        let cdf_len = n * max_actions as usize;
        let cdf: Vec<u8> = (0..cdf_len).map(|i| (i.wrapping_mul(17) % 256) as u8).collect();

        let fh = FileHeader {
            magic: *b"PKRSOTA1",
            version: 1,
            variant_id: 0,
            infoset_count: n as u64,
            max_actions_k: max_actions,
            _padding: [0; 7],
        };
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        write_test_blueprint(&path, &fh, &fmph_hdr, &fmph_bytes, &cdf);

        let mmap = MmapReader::new(&path).unwrap();
        let solver = SolverHandle::new(mmap);

        // Verify all keys return Some with 4 bytes.
        for &k in &keys_vec {
            let advice = solver.get_advice_fast(k).expect("key must be found");
            assert_eq!(advice.cdf_probabilities.len(), max_actions as usize);
            // Optionally check that the slice is within overall CDF? We can't know exact index,
            // but we can check that it's a valid 4-byte slice from the CDF array.
            let slice = &advice.cdf_probabilities;
            let pos = cdf.windows(4).position(|w| w == slice.as_slice());
            assert!(pos.is_some(), "CDF slice for key {k} not found in original CDF");
        }
    }

    #[test]
    fn multiple_levels_mph_evaluation() {
        // Simulate a 2-level MPH. We'll manually construct a 2-level displacement
        // table using a simple approach: For a set of keys, build level1 with one
        // seed and simple displacement, then level2 with another seed to resolve
        // collisions? Our eval_mph simply sums displacements from each level.
        // We'll craft a small 2-level table that maps distinct keys to distinct indices.
        let keys = vec![10u64, 20, 30];
        let n = keys.len();
        let level_count = 2u32;
        let max_level_size = 4usize; // buckets per level

        // Use deterministic seeds for reproducibility.
        let seed = 12345u64;
        // Level 1: displacement array of size max_level_size
        let mut displacements = vec![0u32; max_level_size * level_count as usize];

        // We'll brute-force find per-level displacements for each bucket so that
        // the sum (d1 + d2) % n maps each key to a unique index.
        // Since n=3, we can try small displacements.
        // This is ad-hoc but sufficient for testing the multi-level loop.
        fn eval_test_mph(key: u64, seed: u64, displacements: &[u32], max_level_size: usize, level_count: u32, n: usize) -> usize {
            let mut acc: u64 = 0;
            for l in 0..level_count {
                let level_seed = seed.wrapping_add(l as u64 * 0x9E3779B97F4A7C15);
                let h = hash_key(key, level_seed);
                let bucket = (h % max_level_size as u64) as usize;
                let offset = l as usize * max_level_size + bucket;
                let d = displacements[offset] as u64;
                acc = acc.wrapping_add(d);
            }
            (acc % n as u64) as usize
        }

        // We'll iterate over possible small values to find a working set.
        let mut found = false;
        for d0_0 in 0..4u32 {
            for d0_1 in 0..4u32 {
                for d0_2 in 0..4u32 {
                    for d0_3 in 0..4u32 {
                        for d1_0 in 0..4u32 {
                            for d1_1 in 0..4u32 {
                                for d1_2 in 0..4u32 {
                                    for d1_3 in 0..4u32 {
                                        displacements[0] = d0_0;
                                        displacements[1] = d0_1;
                                        displacements[2] = d0_2;
                                        displacements[3] = d0_3;
                                        displacements[4] = d1_0;
                                        displacements[5] = d1_1;
                                        displacements[6] = d1_2;
                                        displacements[7] = d1_3;
                                        let mut indices = vec![];
                                        for &k in &keys {
                                            indices.push(eval_test_mph(k, seed, &displacements, max_level_size, level_count, n));
                                        }
                                        let set: HashSet<usize> = indices.iter().cloned().collect();
                                        if set.len() == n && indices.iter().all(|&i| i < n) {
                                            found = true;
                                            break;
                                        }
                                    }
                                    if found { break; }
                                }
                                if found { break; }
                            }
                            if found { break; }
                        }
                        if found { break; }
                    }
                    if found { break; }
                }
                if found { break; }
            }
            if found { break; }
        }
        assert!(found, "failed to find 2-level mph");

        let fmp_hdr = FmphHeader {
            num_keys: n as u64,
            seed,
            max_level_size: max_level_size as u64,
            level_count,
            _padding: [0; 4],
        };
        let fmph_data = bytemuck::cast_slice::<u32, u8>(&displacements).to_vec();

        // Build blueprint with 3 infosets, max_actions=2
        let max_actions = 2u8;
        let cdf = vec![10, 20, 30, 40, 50, 60];
        let fh = FileHeader {
            magic: *b"PKRSOTA1",
            version: 1,
            variant_id: 0,
            infoset_count: n as u64,
            max_actions_k: max_actions,
            _padding: [0; 7],
        };
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        write_test_blueprint(&path, &fh, &fmp_hdr, &fmph_data, &cdf);

        let mmap = MmapReader::new(&path).unwrap();
        let solver = SolverHandle::new(mmap);

        // Check each key returns a distinct CDF slice (since mapping is perfect)
        let mut seen_slices = HashSet::new();
        for &k in &keys {
            let advice = solver.get_advice_fast(k).expect("key not found");
            assert_eq!(advice.cdf_probabilities.len(), 2);
            let inserted = seen_slices.insert(advice.cdf_probabilities.clone());
            assert!(inserted, "duplicate CDF slice for key {k}");
        }
    }

    #[test]
    fn blueprint_provider_trait_object_send_sync() {
        // Ensure SolverHandle can be used as trait object and is Send+Sync.
        fn _assert_send_sync<T: Send + Sync>() {}
        _assert_send_sync::<SolverHandle>();

        let keys = vec![1u64];
        let (fmph_hdr, fmph_bytes) = build_test_mph(&keys);
        let fh = FileHeader {
            magic: *b"PKRSOTA1",
            version: 1,
            variant_id: 0,
            infoset_count: 1,
            max_actions_k: 1,
            _padding: [0; 7],
        };
        let cdf = vec![128u8];
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        write_test_blueprint(&path, &fh, &fmph_hdr, &fmph_bytes, &cdf);
        let mmap = MmapReader::new(&path).unwrap();
        let solver = SolverHandle::new(mmap);
        let bp: &dyn BlueprintProvider = &solver;
        let advice = bp.lookup(1).unwrap();
        assert_eq!(advice.cdf_probabilities, vec![128]);
    }

    #[test]
    fn hash_key_deterministic_and_no_panic() {
        let a = hash_key(12345, 67890);
        let b = hash_key(12345, 67890);
        assert_eq!(a, b);
        // edge cases: zeroes
        let _ = hash_key(0, 0);
        // u64::MAX
        let _ = hash_key(u64::MAX, u64::MAX);
    }
}
