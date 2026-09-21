use crate::header::{TranslationTableHeader, FORMAT_VERSION_V2, HASH_ALGO_FNV1A64_INFOSET};
use pkr_cfr::table::CompactRegretTable;
use crate::fmph::build_fmph;
use crate::translate::compute_translation;
use std::fs::File;
use std::io::Write;

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const K: usize = 6;

// Abstract action buckets (6 buckets as defined in traversal.rs):
// 0 = Fold, 1 = Check/Call, 2 = <0.5x pot, 3 = <1.0x pot, 4 = >1.0x pot, 5 = All-in
// Geometric anchor sizes per street (pot fraction):
// On the flop/turn/river, these give well-defined bet/raise anchors.
// Preflop uses a different anchoring (all-in is the only real option).
pub const STREET_BET_FRACTIONS: [[f32; 6]; 4] = [
    // Preflop (street_code 0): no pot-based sizing, all-in anchor
    [0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
    // Flop (street_code 1): 0.45x, 0.9x, 2.2x, all-in
    [0.0, 0.0, 0.45, 0.9, 2.2, 1.0],
    // Turn (street_code 2): same geometric series
    [0.0, 0.0, 0.45, 0.9, 2.2, 1.0],
    // River (street_code 3): same geometric series
    [0.0, 0.0, 0.45, 0.9, 2.2, 1.0],
];

/// Build the pseudo-harmonic translation table for off-tree action mapping.
///
/// For each pair of adjacent abstract bet-size buckets, precompute the
/// translation probabilities for quantized bet fractions.
///
/// Layout: for each (street, lower_bucket, quantized_actual) -> (p_lower_q8, p_upper_q8)
/// Quantization: 256 steps from 0.0 to max bet fraction (2x pot)
pub fn build_translation_table() -> Vec<u8> {
    let num_fractions = 256usize; // quantized bet fractions
    let mut table: Vec<u8> = Vec::with_capacity(4 * K * num_fractions * 2);

    for _street in 0..4u8 {
        for lower_bucket in 0..K {
            for q in 0..num_fractions {
                let actual = q as f32 / num_fractions as f32 * 2.0; // 0.0 to 2.0 pot
                let upper_bucket = if lower_bucket + 1 < K {
                    lower_bucket + 1
                } else {
                    lower_bucket
                };

                // Anchor sizes for this street
                let fractions = STREET_BET_FRACTIONS[0]; // default to flop
                let lower_size = fractions[lower_bucket];
                let upper_size = fractions[upper_bucket];

                if lower_size == 0.0 && upper_size == 0.0 {
                    // Fold/check/call buckets — uniform translation
                    table.push(128);
                    table.push(127);
                    continue;
                }

                if lower_size == upper_size || lower_bucket + 1 >= K {
                    // Single bucket (all-in) — all mass to lower
                    table.push(255);
                    table.push(0);
                    continue;
                }

                // Equal reach probabilities for precomputation (conservative)
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

/// Write a complete blueprint file with FMph, CDF strategies, and translation table.
///
/// File layout:
///   [FileHeader: 32 bytes]
///   [FMphHeader: 40 bytes]
///   [FMph displacements: bucket_count * 4 bytes]
///   [key_count: u32]
///   [cdf_size: u32]
///   [TranslationTableHeader: 16 bytes]
///   [translation table: variable]
///   [sorted keys: num_keys * 8 bytes]
///   [CDF bytes: cdf_size bytes]
pub fn write_blueprint(path: &str, table: &CompactRegretTable, keys: &[u64]) {
    // Build FMph on the REAL infoset hashes
    let fmph = build_fmph(keys);
    let fmph_header = fmph.to_header();

    let num_keys = keys.len();
    let infoset_count = num_keys;

    // Build CDF array for each key, in sorted order
    let mut cdf_bytes: Vec<u8> = Vec::with_capacity(infoset_count * K);
    let mut key_bytes: Vec<u8> = Vec::with_capacity(infoset_count * 8);

    for &key in keys {
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

    let file_header = crate::header::FileHeader {
        magic: *MAGIC,
        version: FORMAT_VERSION_V2,
        variant_id: 0,
        infoset_count: infoset_count as u64,
        max_actions_k: K as u8,
        hash_algo: HASH_ALGO_FNV1A64_INFOSET,
        _padding: [0u8; 6],
    };

    // Build translation table
    let translation_table = build_translation_table();
    let translation_header = TranslationTableHeader {
        num_entries: (translation_table.len() / 2) as u64,
        action_size: 2, // (p_lower, p_upper) per entry
        _padding: [0u8; 4],
    };

    let mut file = File::create(path).expect("failed to create blueprint file");

    // 1. File header
    file.write_all(bytemuck::bytes_of(&file_header)).unwrap();

    // 2. FMph header
    file.write_all(bytemuck::bytes_of(&fmph_header)).unwrap();

    // 3. FMph displacements
    file.write_all(bytemuck::cast_slice(&fmph.displacements))
        .unwrap();

    // 4. Key count + CDF size
    file.write_all(&(num_keys as u32).to_le_bytes()).unwrap();
    file.write_all(&((K * num_keys) as u32).to_le_bytes())
        .unwrap();

    // 5. Translation table header + data
    file.write_all(bytemuck::bytes_of(&translation_header))
        .unwrap();
    file.write_all(&translation_table).unwrap();

    // 6. Key table (sorted)
    file.write_all(&key_bytes).unwrap();

    // 7. CDF strategies
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
        // All-in bucket is always index 5
        for street in &STREET_BET_FRACTIONS {
            assert_eq!(street[5], 1.0, "all-in bucket must be 1.0");
        }
    }

    #[test]
    fn test_translation_table_size() {
        let table = build_translation_table();
        // 4 streets * K buckets * 256 fractions * 2 bytes
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
