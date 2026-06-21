use crate::fmph::build_fmph;
use crate::header::FileHeader;
use pkr_cfr::table::CompactRegretTable;
use std::fs::File;
use std::io::Write;

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const VERSION: u32 = 1;

/// Write the blueprint binary file.
///
/// Layout:
///   FileHeader (32 bytes)
///   FmphDataPacked (64 bytes)
///   Displacements (bucket_count * 4 bytes)
///   CDF array (infoset_count * max_actions_k bytes)
///   TranslationTableHeader (16 bytes)
pub fn write_blueprint(path: &str, table: &CompactRegretTable, keys: &[u64]) {
    let infoset_count = table.capacity();
    let num_actions = table.num_actions();

    // Build minimal perfect hash for the information set keys.
    let fmph = build_fmph(keys);

    // Convert each information set's regret‑based strategy into CDF bytes.
    let mut cdf_bytes: Vec<u8> = Vec::with_capacity(infoset_count * num_actions);
    for infoset_idx in 0..infoset_count {
        let strategy = table.get_strategy(infoset_idx);
        let mut cumulative = 0.0f32;
        for prob in strategy {
            cumulative += prob;
            // Scale to [0,255] and round.
            let byte = (cumulative * 255.0).round().clamp(0.0, 255.0) as u8;
            cdf_bytes.push(byte);
        }
    }

    // Build the file header.
    let file_header = FileHeader {
        magic: *MAGIC,
        version: VERSION,
        variant_id: 0,
        infoset_count: infoset_count as u64,
        max_actions_k: num_actions as u8,
        _padding: [0u8; 7],
    };

    // Pack the FMph for serialisation.
    let mut displacement_bytes = Vec::new();
    let packed_fmph = fmph.pack(&mut displacement_bytes);

    // Translation table – currently empty (no off‑tree translations).
    let translation_header = crate::header::TranslationTableHeader {
        num_entries: 0,
        action_size: 0,
        _padding: [0u8; 4],
    };

    // Write everything to disk.
    let mut file = File::create(path).expect("failed to create blueprint file");
    file.write_all(bytemuck::bytes_of(&file_header))
        .expect("failed to write file header");
    file.write_all(bytemuck::bytes_of(&packed_fmph))
        .expect("failed to write FMph header");
    file.write_all(&displacement_bytes)
        .expect("failed to write displacement array");
    file.write_all(&cdf_bytes)
        .expect("failed to write CDF array");
    file.write_all(bytemuck::bytes_of(&translation_header))
        .expect("failed to write translation table header");
    file.flush().expect("failed to flush blueprint file");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fmph::FmphDataPacked;
    use crate::header::FileHeader;
    use bytemuck;
    use std::fs;
    use std::io::{Read, Seek, SeekFrom};
    use std::path::PathBuf;

    fn create_temp_file_path() -> PathBuf {
        std::env::temp_dir().join(format!("blueprint_test_{}.bin", uuid_simple()))
    }

    fn uuid_simple() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{:x}", t)
    }

    #[test]
    fn test_write_blueprint_header_magic_and_counts() {
        let path = create_temp_file_path();
        let mut table = CompactRegretTable::new(3, 2);
        table.add_regret(0, 0, 10);
        table.add_regret(1, 1, -5);
        let keys: Vec<u64> = vec![100, 200, 300];

        write_blueprint(path.to_str().unwrap(), &table, &keys);

        let mut file = fs::File::open(&path).expect("file should exist");
        let mut header_bytes = [0u8; std::mem::size_of::<FileHeader>()];
        file.read_exact(&mut header_bytes).unwrap();
        let header: &FileHeader = bytemuck::from_bytes(&header_bytes);
        assert_eq!(&header.magic, b"PKRSOTA1");
        assert_eq!(header.version, VERSION);
        assert_eq!(header.infoset_count, 3);
        assert_eq!(header.max_actions_k, 2);
    }

    #[test]
    fn test_write_blueprint_file_size_matches_expectation() {
        let path = create_temp_file_path();
        let mut table = CompactRegretTable::new(3, 2);
        table.add_regret(0, 0, 10);
        let keys: Vec<u64> = vec![100, 200, 300];

        write_blueprint(path.to_str().unwrap(), &table, &keys);

        let metadata = fs::metadata(&path).unwrap();
        // Bucket count = max(keys.len()/2, 1) = 1 → displacements = 4 bytes
        let expected_size = 32          // FileHeader
            + 64                        // FmphDataPacked
            + 4                         // displacements (bucket_count * 4)
            + (3 * 2)                   // CDF array: 3 infosets * 2 actions
            + 16; // TranslationTableHeader (empty)
        assert_eq!(
            metadata.len(),
            expected_size as u64,
            "file size mismatch, expected {expected_size}, got {}",
            metadata.len()
        );
    }

    #[test]
    fn test_write_blueprint_fmph_keys_len() {
        let path = create_temp_file_path();
        let table = CompactRegretTable::new(3, 2);
        let keys: Vec<u64> = vec![100, 200, 300];

        write_blueprint(path.to_str().unwrap(), &table, &keys);

        let mut file = fs::File::open(&path).unwrap();
        // skip FileHeader (32 bytes)
        file.seek(SeekFrom::Start(32)).unwrap();
        let mut fmph_bytes = [0u8; std::mem::size_of::<FmphDataPacked>()];
        file.read_exact(&mut fmph_bytes).unwrap();
        let fmph: &FmphDataPacked = bytemuck::from_bytes(&fmph_bytes);
        assert_eq!(fmph.keys_len, 3);
    }
}
