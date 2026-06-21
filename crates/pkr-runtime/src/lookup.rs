use crate::mmap::MmapReader;
use pkr_contracts::{BlueprintProvider, SotaAdvice};

/// Fast-path runtime lookup engine.
pub struct SolverHandle {
    mmap: MmapReader,
}

impl SolverHandle {
    pub fn new(mmap: MmapReader) -> Self {
        SolverHandle { mmap }
    }

    pub fn get_advice_fast(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        let fh = self.mmap.file_header();
        if fh.infoset_count == 0 {
            return None;
        }

        let num_keys = self.mmap.fmph_header().num_keys as usize;
        let idx = eval_mph(
            infoset_hash,
            num_keys,
            self.mmap.fmph_header(),
            self.mmap.fmph_data(),
        );

        let max_actions = fh.max_actions_k as usize;
        let cdf_start = idx * max_actions;
        let cdf_end = cdf_start + max_actions;
        let cdf_slice = self.mmap.cdf_data();

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

#[inline]
fn hash_key(key: u64, seed: u64) -> u64 {
    key.wrapping_mul(0x9E3779B97F4A7C15) ^ seed
}

/// Evaluates a single‑level Hash‑and‑Displace minimal perfect hash.
fn eval_mph(
    key: u64,
    num_keys: usize,
    fmph_header: &pkr_export::header::FmphHeader,
    fmph_data: &[u8],
) -> usize {
    let level_count = fmph_header.level_count as usize;
    assert_eq!(level_count, 1, "only single‑level MPH is supported");

    let max_level_size = fmph_header.max_level_size as usize;
    let seed1 = fmph_header.seed;
    let seed2 = seed1.wrapping_add(0x9E3779B97F4A7C15);

    let bucket = hash_key(key, seed1) as usize % max_level_size;
    let d = u32::from_le_bytes(fmph_data[bucket * 4..bucket * 4 + 4].try_into().unwrap()) as u64;

    (hash_key(key, seed2).wrapping_add(d) as usize) % num_keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkr_export::header::{FileHeader, FmphHeader, TranslationTableHeader};
    use std::collections::HashSet;
    use std::io::Write;

    /// Build a single‑level displacement‑based MPH for the given keys.
    ///
    /// The displacement array length (`max_level_size`) is always rounded up
    /// to an even number so that its size in bytes is a multiple of 8,
    /// keeping the subsequent TranslationTableHeader aligned.
    fn build_test_mph(keys: &[u64]) -> (FmphHeader, Vec<u8>) {
        assert!(!keys.is_empty(), "must have at least one key");

        let unique: Vec<u64> = {
            let mut set = HashSet::new();
            keys.iter().copied().filter(|k| set.insert(*k)).collect()
        };
        let n = unique.len();
        let bucket_count_raw = (n / 2).max(1);
        // Round up to even so that bucket_count_raw * 4 is multiple of 8.
        let bucket_count = (bucket_count_raw + 1) / 2 * 2;
        let max_displacement = (n as u64 * 8).max(128) as u32;

        let mut rng = simple_rng(42);
        let mut best: Option<(u64, Vec<u32>)> = None;

        for _attempt in 0..10_000 {
            let seed1 = rng.next_u64();
            let seed2 = seed1.wrapping_add(0x9E3779B97F4A7C15);

            let mut buckets: Vec<Vec<u64>> = vec![Vec::new(); bucket_count];
            for &k in &unique {
                let b = hash_key(k, seed1) as usize % bucket_count;
                buckets[b].push(k);
            }

            let mut displacements = vec![0u32; bucket_count];
            let mut used = vec![false; n];
            let mut ok = true;

            let mut perm: Vec<usize> = (0..bucket_count).collect();
            perm.sort_by_key(|&i| buckets[i].len());
            perm.reverse();

            for &b in &perm {
                let bucket_keys = &buckets[b];
                if bucket_keys.is_empty() {
                    continue;
                }
                let mut found = false;
                for d in 0..max_displacement {
                    let mut indices = vec![];
                    let mut collision = false;
                    for &k in bucket_keys {
                        let idx = (hash_key(k, seed2).wrapping_add(d as u64) as usize) % n;
                        if used[idx] || indices.contains(&idx) {
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
                best = Some((seed1, displacements));
                break;
            }
        }

        let (seed, displacements) = best.expect("failed to build MPH after many attempts");
        let hdr = FmphHeader {
            num_keys: n as u64,
            seed,
            max_level_size: bucket_count as u64,
            level_count: 1,
            _padding: [0; 4],
        };
        // displacements is already length `bucket_count` (even).
        let data = bytemuck::cast_slice::<u32, u8>(&displacements).to_vec();
        (hdr, data)
    }

    struct SimpleRng(u64);
    fn simple_rng(seed: u64) -> SimpleRng {
        SimpleRng(seed)
    }
    impl SimpleRng {
        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0
        }
    }

    /// Helper to write a blueprint file.
    ///
    /// The layout matches `MmapReader` expectations:
    ///   FileHeader (32) | FmphHeader (32) | displacements (level_count*max_level_size*4) |
    ///   TranslationTableHeader (16) | CDF data
    ///
    /// No extra padding is added beyond the displacement array as defined.
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

    // -------------------------------------------------------------------
    // Tests
    // -------------------------------------------------------------------

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

        for &key in &keys {
            let advice = solver.get_advice_fast(key).expect("key must be found");
            assert_eq!(advice.cdf_probabilities.len(), max_actions as usize);
            let pos = cdf_bytes
                .windows(max_actions as usize)
                .position(|w| w == advice.cdf_probabilities.as_slice());
            assert!(
                pos.is_some(),
                "CDF slice for key {key} not found in original CDF"
            );
        }
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
        // max_level_size = 2 → 8 bytes displacement (multiple of 8)
        let fmp_hdr = FmphHeader {
            num_keys: 0,
            seed: 0,
            max_level_size: 2,
            level_count: 1,
            _padding: [0; 4],
        };
        let displ = vec![0u8; 8]; // 2 u32 values
        let tt_hdr = TranslationTableHeader {
            num_entries: 0,
            action_size: 0,
            _padding: [0; 4],
        };

        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(bytemuck::bytes_of(&fh)).unwrap();
        f.write_all(bytemuck::bytes_of(&fmp_hdr)).unwrap();
        f.write_all(&displ).unwrap();
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
        // Build MPH for 6 keys, but write a file with infoset_count = 1.
        // Only the key that maps to index 0 should succeed; the rest hit the
        // out‑of‑bounds CDF check and return None.
        let keys_many: Vec<u64> = (0..6).map(|i| i as u64).collect();
        let (fmph_hdr_many, fmph_bytes_many) = build_test_mph(&keys_many);
        let fh = FileHeader {
            magic: *b"PKRSOTA1",
            version: 1,
            variant_id: 0,
            infoset_count: 1, // only one infoset in the file
            max_actions_k: 1,
            _padding: [0; 7],
        };
        let cdf_one = vec![99u8]; // one byte CDF
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_owned();
        write_test_blueprint(&path, &fh, &fmph_hdr_many, &fmph_bytes_many, &cdf_one);

        let mmap = MmapReader::new(&path).unwrap();
        let solver = SolverHandle::new(mmap);

        let mut success_count = 0;
        for k in 0..6u64 {
            if solver.get_advice_fast(k).is_some() {
                success_count += 1;
            }
        }
        assert_eq!(
            success_count, 1,
            "exactly one key should map to index 0 and return Some"
        );
    }

    #[test]
    fn large_keyset_stress_test() {
        let mut rng = simple_rng(999);
        let mut keys_set = HashSet::new();
        while keys_set.len() < 100 {
            keys_set.insert(rng.next_u64());
        }
        let keys_vec: Vec<u64> = keys_set.into_iter().collect();
        let n = keys_vec.len();
        let max_actions = 4u8;
        let (fmph_hdr, fmph_bytes) = build_test_mph(&keys_vec);
        let cdf_len = n * max_actions as usize;
        let cdf: Vec<u8> = (0..cdf_len)
            .map(|i| (i.wrapping_mul(17) % 256) as u8)
            .collect();

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

        for &k in &keys_vec {
            let advice = solver.get_advice_fast(k).expect("key must be found");
            assert_eq!(advice.cdf_probabilities.len(), max_actions as usize);
            let slice = &advice.cdf_probabilities;
            let pos = cdf.windows(4).position(|w| w == slice.as_slice());
            assert!(
                pos.is_some(),
                "CDF slice for key {k} not found in original CDF"
            );
        }
    }

    #[test]
    fn mph_no_collisions() {
        let mut rng = simple_rng(123);
        let keys: Vec<u64> = (0..50).map(|_| rng.next_u64()).collect();
        let n = keys.len();
        let (fmph_hdr, fmph_bytes) = build_test_mph(&keys);

        let mut seen = vec![false; n];
        for &k in &keys {
            let idx = eval_mph(k, n, &fmph_hdr, &fmph_bytes);
            assert!(idx < n, "idx out of range");
            assert!(!seen[idx], "collision at index {idx} for key {k}");
            seen[idx] = true;
        }
        assert!(seen.iter().all(|&x| x), "not all indices used");
    }

    #[test]
    fn blueprint_provider_trait_object_send_sync() {
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
        let _ = hash_key(0, 0);
        let _ = hash_key(u64::MAX, u64::MAX);
    }
}
