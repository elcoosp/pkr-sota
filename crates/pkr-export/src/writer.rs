use crate::fmph::{build_fmph, eval_fmph, FmphData};
use crate::header::FileHeader;
use pkr_cfr::table::CompactRegretTable;
use std::fs::File;
use std::io::Write;

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const VERSION: u32 = 1;
const K: usize = 6;

pub fn write_blueprint(path: &str, table: &CompactRegretTable) {
    let keys = table.get_keys();
    let infoset_count = keys.len();

    let fmph: FmphData = build_fmph(&keys);

    let mut cdf_indexed = vec![0u8; infoset_count * K];
    let mut key_table = vec![0u64; infoset_count];
    for &key in &keys {
        let idx = eval_fmph(&fmph, key);
        key_table[idx] = key;
        let strategy_slice = table.get_average_strategy_slice(key);
        let mut cumulative = 0.0f32;
        for a in 0..K {
            let prob = strategy_slice.map(|s| s[a]).unwrap_or(1.0 / K as f32);
            cumulative += prob;
            let byte = (cumulative * 255.0).round().clamp(0.0, 255.0) as u8;
            cdf_indexed[idx * K + a] = byte;
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

    let fmph_header = fmph.to_header();

    let translation_header = crate::header::TranslationTableHeader {
        num_entries: 0,
        action_size: 0,
        _padding: [0u8; 4],
    };

    let mut file = File::create(path).expect("failed to create blueprint file");
    file.write_all(bytemuck::bytes_of(&file_header)).unwrap();
    file.write_all(bytemuck::bytes_of(&fmph_header)).unwrap();
    file.write_all(bytemuck::cast_slice(&fmph.displacements)).unwrap();
    file.write_all(bytemuck::bytes_of(&translation_header)).unwrap();
    file.write_all(bytemuck::cast_slice(&key_table)).unwrap();
    file.write_all(&cdf_indexed).unwrap();
    file.flush().unwrap();
}
