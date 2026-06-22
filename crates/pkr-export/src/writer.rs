use crate::fmph::{build_fmph, eval_fmph, FmphData};
use crate::header::FileHeader;
use pkr_cfr::table::CompactRegretTable;
use std::fs::File;
use std::io::Write;

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const VERSION: u32 = 1;

pub fn write_blueprint(path: &str, table: &CompactRegretTable) {
    let keys = table.get_keys();
    let infoset_count = keys.len();
    let num_actions = table.num_actions();

    let fmph: FmphData = build_fmph(&keys);

    // CDF indexed by MPH displacement order
    let mut cdf_indexed = vec![0u8; infoset_count * num_actions];
    // Key verification table (stores the original key at each MPH index)
    let mut key_table = vec![0u64; infoset_count];
    for &key in &keys {
        let idx = eval_fmph(&fmph, key);
        key_table[idx] = key;
        let strategy = table.get_average_strategy(key);
        let mut cumulative = 0.0f32;
        for (a, &prob) in strategy.iter().enumerate() {
            cumulative += prob;
            let byte = (cumulative * 255.0).round().clamp(0.0, 255.0) as u8;
            cdf_indexed[idx * num_actions + a] = byte;
        }
    }

    let file_header = FileHeader {
        magic: *MAGIC,
        version: VERSION,
        variant_id: 0,
        infoset_count: infoset_count as u64,
        max_actions_k: num_actions as u8,
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
    // Write key verification table (u64 per infoset)
    file.write_all(bytemuck::cast_slice(&key_table)).unwrap();
    // Write CDF data
    file.write_all(&cdf_indexed).unwrap();
    file.flush().unwrap();
}
