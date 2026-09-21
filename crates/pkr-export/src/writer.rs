use crate::header::{FORMAT_VERSION_V2, HASH_ALGO_FNV1A64_INFOSET};
use crate::translate::compute_translation;
use pkr_cfr::table::CompactRegretTable;
use std::fs::File;
use std::io::Write;

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const K: usize = 6;

pub const STREET_BET_FRACTIONS: [[f32; 6]; 4] = [
    [0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
    [0.0, 0.0, 0.45, 0.9, 2.2, 1.0],
    [0.0, 0.0, 0.45, 0.9, 2.2, 1.0],
    [0.0, 0.0, 0.45, 0.9, 2.2, 1.0],
];

pub fn build_translation_table() -> Vec<u8> {
    let num_fractions = 256usize;
    let mut table: Vec<u8> = Vec::with_capacity(4 * K * num_fractions * 2);

    for _street in 0..4u8 {
        for lower_bucket in 0..K {
            for q in 0..num_fractions {
                let actual = q as f32 / num_fractions as f32 * 2.0;
                let upper_bucket = if lower_bucket + 1 < K {
                    lower_bucket + 1
                } else {
                    lower_bucket
                };

                let fractions = STREET_BET_FRACTIONS[0];
                let lower_size = fractions[lower_bucket];
                let upper_size = fractions[upper_bucket];

                if lower_size == 0.0 && upper_size == 0.0 {
                    table.push(128);
                    table.push(127);
                    continue;
                }

                if lower_size == upper_size || lower_bucket + 1 >= K {
                    table.push(255);
                    table.push(0);
                    continue;
                }

                let reach_lower = 0.5;
                let reach_upper = 0.5;

                let (p_lower, p_upper) =
                    compute_translation(lower_size, upper_size, actual, reach_lower, reach_upper);
                table.push(p_lower);
                table.push(p_upper);
            }
        }
    }

    table
}

pub fn write_blueprint(path: &str, table: &CompactRegretTable, keys: &[u64]) {
    let mut sorted_keys: Vec<u64> = keys.to_vec();
    sorted_keys.sort_unstable();
    let keys: &[u64] = &sorted_keys;
    let num_keys = keys.len();
    let infoset_count = num_keys;

    let mut cdf_bytes: Vec<u8> = Vec::with_capacity(infoset_count * K);
    let mut key_bytes: Vec<u8> = Vec::with_capacity(infoset_count * 8);

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

    let file_header = crate::header::FileHeader {
        magic: *MAGIC,
        version: FORMAT_VERSION_V2,
        variant_id: 0,
        infoset_count: infoset_count as u64,
        max_actions_k: K as u8,
        hash_algo: HASH_ALGO_FNV1A64_INFOSET,
        _padding: [0u8; 6],
    };

    let mut file = File::create(path).expect("failed to create blueprint file");

    file.write_all(bytemuck::bytes_of(&file_header)).unwrap();
    file.write_all(&(num_keys as u32).to_le_bytes()).unwrap();
    file.write_all(&((K * num_keys) as u32).to_le_bytes()).unwrap();
    file.write_all(&key_bytes).unwrap();
    file.write_all(&cdf_bytes).unwrap();

    file.flush().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_street_bet_fractions_structure() {
        assert_eq!(STREET_BET_FRACTIONS.len(), 4);
        for street in &STREET_BET_FRACTIONS {
            assert_eq!(street.len(), K);
        }
        for street in &STREET_BET_FRACTIONS {
            assert_eq!(street[5], 1.0, "all-in bucket must be 1.0");
        }
    }

    #[test]
    fn test_translation_table_size() {
        let table = build_translation_table();
        assert_eq!(table.len(), 4 * K * 256 * 2);
    }

    #[test]
    fn test_translation_table_sums_to_255() {
        let table = build_translation_table();
        for chunk in table.chunks(2) {
            let sum = chunk[0] as u16 + chunk[1] as u16;
            assert_eq!(sum, 255, "translation probs must sum to 255");
        }
    }
}
