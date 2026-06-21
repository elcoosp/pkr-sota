use pkr_cfr::table::CompactRegretTable;

/// Stub – will be implemented in green phase.
pub fn write_blueprint(_path: &str, _table: &CompactRegretTable, _keys: &[u64]) {
    unimplemented!("green phase not yet implemented")
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
        // Arrange
        let path = create_temp_file_path();
        let mut table = CompactRegretTable::new(3, 2);
        table.add_regret(0, 0, 10);
        table.add_regret(1, 1, -5);
        let keys: Vec<u64> = vec![100, 200, 300];

        // Act
        write_blueprint(path.to_str().unwrap(), &table, &keys);

        // Assert
        let mut file = fs::File::open(&path).expect("file should exist");
        let mut header_bytes = [0u8; std::mem::size_of::<FileHeader>()];
        file.read_exact(&mut header_bytes).unwrap();
        let header: &FileHeader = bytemuck::from_bytes(&header_bytes);
        assert_eq!(&header.magic, b"PKRSOTA1");
        assert_eq!(header.version, 1);
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
        let expected_size = 32          // FileHeader
            + 64                        // FmphDataPacked
            + ( (keys.len()/2).max(1) * 4 )  // displacements (u32 per bucket)
            + (3 * 2)                   // CDF array: 3 infosets * 2 actions * 1 byte
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
