use crate::header::{
    AnchorsSection, FileHeader, ANCHORS, FORMAT_VERSION_V4, HASH_ALGO_FNV1A64_INFOSET,
};
use pkr_cfr::table::CompactRegretTable;
use pkr_core::abstraction::AbstractionFingerprint;
use std::fs::File;
use std::io::Write;

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const K: usize = 6;

/// Write a complete blueprint file.
///
/// v4 layout:
///   [FileHeader:32][AnchorsSection:48][Fingerprint:40][key_count:u32][cdf_size:u32][keys][cdf]
///
/// Keys are u64 ascending; CDFs are K bytes per key with monotonic
/// non-decreasing values ending at 255.
pub fn write_blueprint(
    path: &str,
    table: &CompactRegretTable,
    keys: &[u64],
    fingerprint: &AbstractionFingerprint,
) {
    // Sort defensively (reader uses binary search).
    let mut sorted_keys: Vec<u64> = keys.to_vec();
    sorted_keys.sort_unstable();
    let keys: &[u64] = &sorted_keys;
    let num_keys = keys.len();

    let mut cdf_bytes: Vec<u8> = Vec::with_capacity(num_keys * K);
    let mut key_bytes: Vec<u8> = Vec::with_capacity(num_keys * 8);

    let mut strat = [0.0f32; K];
    for &key in keys {
        key_bytes.extend_from_slice(&key.to_le_bytes());
        table.get_average_strategy_into(key, &mut strat);
        let mut cumulative = 0.0f32;
        for a in 0..K {
            cumulative += strat[a];
            let byte = (cumulative * 255.0).round().clamp(0.0, 255.0) as u8;
            cdf_bytes.push(byte);
        }
    }

    let file_header = FileHeader {
        magic: *MAGIC,
        version: FORMAT_VERSION_V4,
        variant_id: 0,
        infoset_count: num_keys as u64,
        max_actions_k: K as u8,
        hash_algo: HASH_ALGO_FNV1A64_INFOSET,
        _padding: [0u8; 6],
    };

    let anchors = AnchorsSection { anchors: ANCHORS };

    let mut file = File::create(path).expect("failed to create blueprint file");

    // 1. FileHeader (32 B)
    file.write_all(bytemuck::bytes_of(&file_header)).unwrap();
    // 2. AnchorsSection (48 B)
    file.write_all(bytemuck::bytes_of(&anchors)).unwrap();
    // 3. AbstractionFingerprint (40 B) — F2c
    file.write_all(bytemuck::bytes_of(fingerprint)).unwrap();
    // 4. key_count:u32, cdf_size:u32
    file.write_all(&(num_keys as u32).to_le_bytes()).unwrap();
    file.write_all(&((K * num_keys) as u32).to_le_bytes())
        .unwrap();
    // 5. keys
    file.write_all(&key_bytes).unwrap();
    // 6. cdfs
    file.write_all(&cdf_bytes).unwrap();

    file.flush().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_section_size_is_48() {
        assert_eq!(std::mem::size_of::<AnchorsSection>(), 48);
    }

    #[test]
    fn file_header_size_is_32() {
        assert_eq!(std::mem::size_of::<FileHeader>(), 32);
    }
}

#[cfg(test)]
mod v4_layout_tests {
    use super::*;
    use pkr_cfr::table::CompactRegretTable;

    /// The v4 blueprint file size is a deterministic function of the
    /// key count:
    ///
    ///   [FileHeader:32][Anchors:48][Fingerprint:40][kc:4][cs:4][keys:8N][cdf:6N]
    ///   = 128 + 14N
    ///
    /// This pins the on-disk layout. If any section size changes, this
    /// test fails and the format version must be bumped.
    #[test]
    fn v4_size_accounting_is_exact() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let table = CompactRegretTable::with_capacity(64);
        // Touch a few keys so get_average_strategy_into returns real CDFs.
        for k in [1u64, 42, 999] {
            table.add_strategy_sum(k, 0, 0.5);
            table.add_strategy_sum(k, 1, 0.5);
        }
        let keys: Vec<u64> = vec![1, 42, 999];
        let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(4);

        write_blueprint(tmp.path().to_str().unwrap(), &table, &keys, &fp);

        let expected = 32 + 48 + 40 + 4 + 4 + 8 * keys.len() + 6 * keys.len();
        let actual = std::fs::metadata(tmp.path()).unwrap().len() as usize;
        assert_eq!(
            actual,
            expected,
            "v4 blueprint size mismatch: got {}, expected {} (N={})",
            actual,
            expected,
            keys.len()
        );
    }

    /// Section offsets are stable: reading back the fingerprint must
    /// yield the one written.
    #[test]
    fn v4_fingerprint_roundtrips() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let table = CompactRegretTable::with_capacity(16);
        let keys: Vec<u64> = vec![100];
        let fp = pkr_core::abstraction::AbstractionFingerprint::from_constants(7);
        write_blueprint(tmp.path().to_str().unwrap(), &table, &keys, &fp);

        let bytes = std::fs::read(tmp.path()).unwrap();
        // Fingerprint at offset 32 + 48 = 80, length 40.
        let fp_bytes = &bytes[80..120];
        let stored: &pkr_core::abstraction::AbstractionFingerprint = bytemuck::from_bytes(fp_bytes);
        assert_eq!(*stored, fp);
    }
}
