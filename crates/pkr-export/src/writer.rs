use pkr_cfr::table::CompactRegretTable;
use std::fs::File;
use std::io::Write;
use crate::header::FileHeader;

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const VERSION: u32 = 1;
const K: usize = 6;

pub fn write_blueprint(path: &str, table: &CompactRegretTable) {
    let mut keys = table.get_keys();
    keys.sort_unstable();

    let infoset_count = keys.len();

    // CDF array for each key, in sorted order
    let mut cdf_bytes: Vec<u8> = Vec::with_capacity(infoset_count * K);
    let mut key_bytes: Vec<u8> = Vec::with_capacity(infoset_count * 8);

    for &key in &keys {
        key_bytes.extend_from_slice(&key.to_le_bytes());
        let strategy_slice = table.get_average_strategy_slice(key);
        let mut cumulative = 0.0f32;
        for a in 0..K {
            let prob = strategy_slice.map(|s| s[a]).unwrap_or(1.0 / K as f32);
            cumulative += prob;
            let byte = (cumulative * 255.0).round().clamp(0.0, 255.0) as u8;
            cdf_bytes.push(byte);
        }
    }

    let file_header = FileHeader {
        magic: *MAGIC,
        version: VERSION,
        variant_id: 0,
        infoset_count: infoset_count as u64,
        max_actions_k: K as u8,
        _padding: [0u8; 7],
    };

    // No FMPH, no translation table – we put a zero-length placeholder for format compat
    let mut file = File::create(path).expect("failed to create blueprint file");
    file.write_all(bytemuck::bytes_of(&file_header)).unwrap();
    // Write key table size (u32) and CDF size (u32) for simple parsing
    file.write_all(&(infoset_count as u32).to_le_bytes()).unwrap();
    file.write_all(&((K * infoset_count) as u32).to_le_bytes()).unwrap();
    file.write_all(&key_bytes).unwrap();
    file.write_all(&cdf_bytes).unwrap();
    file.flush().unwrap();
}
