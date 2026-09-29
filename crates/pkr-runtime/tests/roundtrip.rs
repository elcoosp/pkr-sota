use pkr_contracts::BlueprintProvider;
use pkr_runtime::{MmapReader, SolverHandle};

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const FORMAT_VERSION_V2: u32 = 2;
const HASH_ALGO_FNV1A64_INFOSET: u8 = 2;
const K: usize = 6;

fn write_synthetic_blueprint(path: &std::path::Path, keys: &[u64], cdfs: &[[u8; K]]) {
    assert_eq!(keys.len(), cdfs.len(), "keys and cdfs must match");
    let n = keys.len();

    let mut bytes: Vec<u8> = Vec::with_capacity(32 + 8 + n * 8 + n * K);

    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&FORMAT_VERSION_V2.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(n as u64).to_le_bytes());
    bytes.push(K as u8);
    bytes.push(HASH_ALGO_FNV1A64_INFOSET);
    bytes.extend_from_slice(&[0u8; 6]);
    assert_eq!(bytes.len(), 32, "FileHeader must be exactly 32 bytes");

    bytes.extend_from_slice(&(n as u32).to_le_bytes());
    bytes.extend_from_slice(&((n * K) as u32).to_le_bytes());

    for &k in keys {
        bytes.extend_from_slice(&k.to_le_bytes());
    }
    for c in cdfs {
        bytes.extend_from_slice(c);
    }

    std::fs::write(path, &bytes).unwrap();
}

#[test]
fn roundtrip_lookup_hits_and_misses() {
    let tmp = tempfile::NamedTempFile::new().unwrap();

    let mut keys: [u64; 3] = [
        0xAAAA_BBBB_CCCC_DDDDu64,
        0x1111_2222_3333_4444u64,
        0x5555_6666_7777_8888u64,
    ];
    keys.sort_unstable();

    let by_key: std::collections::HashMap<u64, [u8; K]> = [
        (0x1111_2222_3333_4444u64, [10u8, 40, 80, 150, 200, 255]),
        (0x5555_6666_7777_8888u64, [42u8, 85, 128, 170, 212, 255]),
        (0xAAAA_BBBB_CCCC_DDDDu64, [255u8, 255, 255, 255, 255, 255]),
    ]
    .into_iter()
    .collect();

    let cdfs: Vec<[u8; K]> = keys.iter().map(|k| by_key[k]).collect();
    write_synthetic_blueprint(tmp.path(), &keys, &cdfs);

    let reader = MmapReader::new(tmp.path()).unwrap();
    assert_eq!(reader.file_header().infoset_count, 3);
    assert_eq!(reader.file_header().max_actions_k, K as u8);
    assert_eq!(reader.keys_data().len(), 3 * 8);
    assert_eq!(reader.cdf_data().len(), 3 * K);

    let handle = SolverHandle::new(reader);

    for &k in &keys {
        let advice = handle
            .get_advice_fast(k)
            .unwrap_or_else(|| panic!("hit expected for key {k:#x}"));
        assert_eq!(advice.len as usize, K);
        assert_eq!(&advice.cdf_probabilities[..K], &by_key[&k][..]);
        for j in 1..K {
            assert!(
                advice.cdf_probabilities[j] >= advice.cdf_probabilities[j - 1],
                "CDF must be monotonic non-decreasing, got {:?}",
                &advice.cdf_probabilities[..K]
            );
        }
        assert_eq!(advice.cdf_probabilities[K - 1], 255);
        assert!(
            handle.lookup(k).is_some(),
            "BlueprintProvider::lookup must agree with get_advice_fast"
        );
    }

    assert!(
        handle.get_advice_fast(0).is_none(),
        "miss expected for key 0"
    );
    assert!(
        handle.get_advice_fast(0xFFFF_FFFF_FFFF_FFFF).is_none(),
        "miss expected for key 0xFFFF_FFFF_FFFF_FFFF"
    );
}

#[test]
fn roundtrip_empty_blueprint_is_safe() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let keys: [u64; 0] = [];
    let cdfs: [[u8; K]; 0] = [];
    write_synthetic_blueprint(tmp.path(), &keys, &cdfs);

    let reader = MmapReader::new(tmp.path()).unwrap();
    assert_eq!(reader.file_header().infoset_count, 0);
    let handle = SolverHandle::new(reader);
    assert!(handle.get_advice_fast(0xDEAD_BEEF).is_none());
}

#[test]
fn roundtrip_cdf_survives_byte_for_byte() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let keys = [42u64];
    let cdfs = [[7u8, 33, 99, 150, 210, 255]];
    write_synthetic_blueprint(tmp.path(), &keys, &cdfs);

    let reader = MmapReader::new(tmp.path()).unwrap();
    let handle = SolverHandle::new(reader);
    let advice = handle.get_advice_fast(42).unwrap();
    assert_eq!(&advice.cdf_probabilities[..K], &cdfs[0][..]);
}

/// The fallback advice must produce a valid non-degenerate CDF:
/// monotonic non-decreasing, last byte = 255, len = 6, first byte > 0
/// and < 200 (so it is neither all-fold nor never-fold).
#[test]
fn fallback_advice_is_safe() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let keys: [u64; 0] = [];
    let cdfs: [[u8; K]; 0] = [];
    write_synthetic_blueprint(tmp.path(), &keys, &cdfs);
    let reader = MmapReader::new(tmp.path()).unwrap();
    let handle = SolverHandle::new(reader);
    let fb = handle.fallback_advice();
    assert_eq!(fb.len, 6);
    for i in 1..6 {
        assert!(
            fb.cdf_probabilities[i] >= fb.cdf_probabilities[i - 1],
            "CDF must be monotonic"
        );
    }
    assert_eq!(fb.cdf_probabilities[5], 255);
    assert!(
        fb.cdf_probabilities[0] > 0,
        "must have some fold probability"
    );
    assert!(fb.cdf_probabilities[0] < 200, "must not be all-fold");
}

#[test]
fn batch_matches_single_lookup_byte_for_byte() {
    let tmp = tempfile::NamedTempFile::new().unwrap();

    // Enough keys to exercise the merge walk across segments.
    let mut keys: Vec<u64> = (0u64..64)
        .map(|i| 0x1111_0000_0000_0000u64 + i * 0x0101_0101_0101_0101)
        .collect();
    keys.sort_unstable();

    let cdfs: Vec<[u8; K]> = (0..keys.len())
        .map(|i| {
            let mut c = [0u8; K];
            for j in 0..K {
                c[j] = ((i + j * 7) % 256) as u8;
            }
            c[K - 1] = 255;
            c
        })
        .collect();
    write_synthetic_blueprint(tmp.path(), &keys, &cdfs);

    let reader = MmapReader::new(tmp.path()).unwrap();
    let handle = SolverHandle::new(reader);

    // Lookups: some hit, some miss, some hit again (duplicates), all
    // unsorted. Order in input must be preserved in output.
    let mut queries: Vec<u64> = Vec::new();
    queries.push(keys[0]);
    queries.push(0xDEAD_BEEF_DEAD_BEEFu64); // miss
    queries.push(keys[32]);
    queries.push(keys[0]); // duplicate hit
    queries.push(keys[63]);
    queries.push(keys[63]); // duplicate hit
    queries.push(0x0);      // miss (below)
    queries.push(u64::MAX); // miss (above)

    let batch = handle.get_advice_batch(&queries);
    assert_eq!(batch.len(), queries.len());

    for (i, &h) in queries.iter().enumerate() {
        let single = handle.get_advice_fast(h);
        assert_eq!(
            batch[i].is_some(),
            single.is_some(),
            "presence differs at index {i} for hash 0x{h:016x}"
        );
        if let (Some(b), Some(s)) = (&batch[i], &single) {
            assert_eq!(
                b.len, s.len,
                "len differs at index {i} for hash 0x{h:016x}"
            );
            assert_eq!(
                &b.cdf_probabilities[..b.len as usize],
                &s.cdf_probabilities[..s.len as usize],
                "cdf differs at index {i} for hash 0x{h:016x}"
            );
        }
    }
}

#[test]
fn batch_handles_empty_input() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let keys: Vec<u64> = vec![1, 2, 3];
    let cdfs: Vec<[u8; K]> = vec![[0; K]; 3];
    write_synthetic_blueprint(tmp.path(), &keys, &cdfs);

    let reader = MmapReader::new(tmp.path()).unwrap();
    let handle = SolverHandle::new(reader);

    let empty: &[u64] = &[];
    assert!(handle.get_advice_batch(empty).is_empty());
}

#[test]
fn batch_with_all_misses_returns_all_none() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let keys: Vec<u64> = vec![0x100, 0x200, 0x300];
    let cdfs: Vec<[u8; K]> = vec![[0; K]; 3];
    write_synthetic_blueprint(tmp.path(), &keys, &cdfs);

    let reader = MmapReader::new(tmp.path()).unwrap();
    let handle = SolverHandle::new(reader);

    let queries: Vec<u64> = vec![1, 0x150, 0x250, 0x350, u64::MAX];
    let batch = handle.get_advice_batch(&queries);
    assert!(batch.iter().all(|r| r.is_none()));
}

#[test]
fn batch_with_all_hits_returns_all_some() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let keys: Vec<u64> = vec![0x100, 0x200, 0x300];
    let cdfs: Vec<[u8; K]> = vec![
        [10, 40, 80, 150, 200, 255],
        [42, 85, 128, 170, 212, 255],
        [255, 255, 255, 255, 255, 255],
    ];
    write_synthetic_blueprint(tmp.path(), &keys, &cdfs);

    let reader = MmapReader::new(tmp.path()).unwrap();
    let handle = SolverHandle::new(reader);

    let queries: Vec<u64> = vec![0x100, 0x200, 0x300];
    let batch = handle.get_advice_batch(&queries);
    assert!(batch.iter().all(|r| r.is_some()));
    for (i, r) in batch.iter().enumerate() {
        let a = r.as_ref().unwrap();
        let cdf = &a.cdf_probabilities[..a.len as usize];
        assert_eq!(cdf, &cdfs[i][..a.len as usize]);
    }
}
