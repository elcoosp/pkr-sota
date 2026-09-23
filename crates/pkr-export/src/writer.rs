use crate::header::{
    AnchorsSection, FileHeader, ANCHORS, FORMAT_VERSION_V3, HASH_ALGO_FNV1A64_INFOSET,
};
use pkr_cfr::table::CompactRegretTable;
use std::fs::File;
use std::io::Write;

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const K: usize = 6;

/// Write a complete blueprint file.
///
/// v3 layout:
///   [FileHeader:32][AnchorsSection:48][key_count:u32][cdf_size:u32][keys][cdf]
///
/// Keys are u64 ascending; CDFs are K bytes per key with monotonic
/// non-decreasing values ending at 255.
pub fn write_blueprint(path: &str, table: &CompactRegretTable, keys: &[u64]) {
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
        version: FORMAT_VERSION_V3,
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
    // 3. key_count:u32, cdf_size:u32
    file.write_all(&(num_keys as u32).to_le_bytes()).unwrap();
    file.write_all(&((K * num_keys) as u32).to_le_bytes())
        .unwrap();
    // 4. keys
    file.write_all(&key_bytes).unwrap();
    // 5. cdfs
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
