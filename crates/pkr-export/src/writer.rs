use crate::fmph::build_fmph;
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

    let fmph = build_fmph(&keys);

    let mut cdf_bytes: Vec<u8> = Vec::with_capacity(infoset_count * num_actions);
    for &key in &keys {
        let strategy = table.get_average_strategy(key);
        let mut cumulative = 0.0f32;
        for prob in strategy {
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
        max_actions_k: num_actions as u8,
        _padding: [0u8; 7],
    };

    let fmph_header = crate::header::FmphHeader {
        num_keys: fmph.keys_len as u64,
        seed1: fmph.seed1,
        seed2: fmph.seed2,
        max_level_size: fmph.bucket_count as u64,
        level_count: 1,
        _padding: [0u8; 4],
    };

    let translation_header = crate::header::TranslationTableHeader {
        num_entries: 0,
        action_size: 0,
        _padding: [0u8; 4],
    };

    let mut file = File::create(path).expect("failed to create blueprint file");
    file.write_all(bytemuck::bytes_of(&file_header)).unwrap();
    file.write_all(bytemuck::bytes_of(&fmph_header)).unwrap();
    file.write_all(bytemuck::cast_slice(&fmph.displacements))
        .unwrap();
    file.write_all(bytemuck::bytes_of(&translation_header))
        .unwrap();
    file.write_all(&cdf_bytes).unwrap();
    file.flush().unwrap();
}
