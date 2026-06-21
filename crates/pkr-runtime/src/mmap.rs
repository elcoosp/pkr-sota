use std::fs::File;
use std::path::Path;

use bytemuck::{Pod, Zeroable};
use memmap2::Mmap;
use pkr_export::header::{FileHeader, FmphHeader, TranslationTableHeader};
use thiserror::Error;

/// Errors that can occur when opening or parsing a blueprint file.
#[derive(Error, Debug)]
pub enum MmapError {
    #[error("failed to open blueprint file: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid magic bytes: expected {expected:?}, got {actual:?}")]
    InvalidMagic { expected: [u8; 8], actual: [u8; 8] },

    #[error("unsupported version: {0}")]
    UnsupportedVersion(u32),

    #[error("file too small for header")]
    FileTooSmall,

    #[error("invalid section offset: {0}")]
    InvalidOffset(&'static str),
}

/// Magic constant expected at the beginning of every blueprint file.
const MAGIC: &[u8; 8] = b"PKRSOTA1";
/// Supported format version.
const SUPPORTED_VERSION: u32 = 1;

/// A read-only, memory-mapped view of a `blueprint.bin` file.
///
/// # Layout
///
/// ```text
/// FileHeader                (32 bytes)
/// FmphHeader                (32 bytes)
/// Fmph displacement data    (level_count * max_level_size * 4 bytes)
/// TranslationTableHeader    (16 bytes)
/// Translation table entries (num_entries * action_size bytes)
/// CDF data                  (infoset_count * max_actions_k bytes)
/// ```
pub struct MmapReader {
    /// The memory-mapped file.
    _mmap: Mmap,
    /// Pointer to the parsed file header (points into `_mmap`).
    file_header: *const FileHeader,
    /// Offset to the Fmph header from the start of the file.
    offset_fmph_header: usize,
    /// Offset to the raw Fmph displacement data.
    offset_fmph_data: usize,
    /// Length of the Fmph displacement data in bytes.
    len_fmph_data: usize,
    /// Offset to the translation table header.
    offset_translation_header: usize,
    /// Offset to the raw translation table entries.
    offset_translation_data: usize,
    /// Length of the translation table data in bytes.
    len_translation_data: usize,
    /// Offset to the CDF data.
    offset_cdf: usize,
    /// Length of the CDF data in bytes.
    len_cdf: usize,
}

// Mmap is Send + Sync, and we only ever read from the file.
unsafe impl Send for MmapReader {}
unsafe impl Sync for MmapReader {}

impl MmapReader {
    /// Opens the file at `path`, memory-maps it, and validates / parses the headers.
    ///
    /// # Errors
    ///
    /// Returns `MmapError` if the file cannot be opened, is too small, has an invalid
    /// magic number, or uses an unsupported version.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, MmapError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        if mmap.len() < std::mem::size_of::<FileHeader>() {
            return Err(MmapError::FileTooSmall);
        }

        // Safety: mmap is at least as large as FileHeader and is valid for reads.
        let file_header: &FileHeader =
            bytemuck::from_bytes(&mmap[..std::mem::size_of::<FileHeader>()]);

        // Validate magic.
        if &file_header.magic != MAGIC {
            return Err(MmapError::InvalidMagic {
                expected: *MAGIC,
                actual: file_header.magic,
            });
        }

        // Validate version.
        if file_header.version != SUPPORTED_VERSION {
            return Err(MmapError::UnsupportedVersion(file_header.version));
        }

        // --- Compute offsets ---

        let offset_fmph_header = std::mem::size_of::<FileHeader>();
        let offset_fmph_data = offset_fmph_header + std::mem::size_of::<FmphHeader>();

        // Need to read FmphHeader to know data size.
        let fmph_header_end = offset_fmph_header + std::mem::size_of::<FmphHeader>();
        if mmap.len() < fmph_header_end {
            return Err(MmapError::FileTooSmall);
        }
        let fmph_header: &FmphHeader =
            bytemuck::from_bytes(&mmap[offset_fmph_header..fmph_header_end]);

        let len_fmph_data = fmph_header.level_count as u64 * fmph_header.max_level_size as u64 * 4;
        let len_fmph_data = len_fmph_data as usize;

        let offset_translation_header = offset_fmph_data + len_fmph_data;
        let offset_translation_data =
            offset_translation_header + std::mem::size_of::<TranslationTableHeader>();

        // Read TranslationTableHeader to know data size.
        let tt_header_end =
            offset_translation_header + std::mem::size_of::<TranslationTableHeader>();
        if mmap.len() < tt_header_end {
            return Err(MmapError::FileTooSmall);
        }
        let tt_header: &TranslationTableHeader =
            bytemuck::from_bytes(&mmap[offset_translation_header..tt_header_end]);

        let len_translation_data = tt_header.num_entries as u64 * tt_header.action_size as u64;
        let len_translation_data = len_translation_data as usize;

        let offset_cdf = offset_translation_data + len_translation_data;
        let len_cdf = file_header.infoset_count as usize * file_header.max_actions_k as usize;

        let total_required = offset_cdf + len_cdf;
        if mmap.len() < total_required {
            return Err(MmapError::InvalidOffset(
                "CDF data extends past end of file",
            ));
        }

        // Obtain a stable pointer to the file header for safe access later.
        let file_header_ptr: *const FileHeader = file_header as *const FileHeader;

        Ok(MmapReader {
            _mmap: mmap,
            file_header: file_header_ptr,
            offset_fmph_header,
            offset_fmph_data,
            len_fmph_data,
            offset_translation_header,
            offset_translation_data,
            len_translation_data,
            offset_cdf,
            len_cdf,
        })
    }

    /// Returns a reference to the parsed `FileHeader`.
    #[inline]
    pub fn file_header(&self) -> &FileHeader {
        // Safety: pointer is valid for the lifetime of the Mmap (self._mmap).
        unsafe { &*self.file_header }
    }

    /// Returns a reference to the `FmphHeader`.
    #[inline]
    pub fn fmph_header(&self) -> &FmphHeader {
        let slice = &self._mmap
            [self.offset_fmph_header..self.offset_fmph_header + std::mem::size_of::<FmphHeader>()];
        bytemuck::from_bytes(slice)
    }

    /// Returns the raw bytes of the Fmph displacement data.
    #[inline]
    pub fn fmph_data(&self) -> &[u8] {
        &self._mmap[self.offset_fmph_data..self.offset_fmph_data + self.len_fmph_data]
    }

    /// Returns a reference to the `TranslationTableHeader`.
    #[inline]
    pub fn translation_table_header(&self) -> &TranslationTableHeader {
        let slice = &self._mmap[self.offset_translation_header
            ..self.offset_translation_header + std::mem::size_of::<TranslationTableHeader>()];
        bytemuck::from_bytes(slice)
    }

    /// Returns the raw bytes of the translation table.
    #[inline]
    pub fn translation_table_data(&self) -> &[u8] {
        &self._mmap
            [self.offset_translation_data..self.offset_translation_data + self.len_translation_data]
    }

    /// Returns the raw bytes of the CDF data.
    #[inline]
    pub fn cdf_data(&self) -> &[u8] {
        &self._mmap[self.offset_cdf..self.offset_cdf + self.len_cdf]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::mem::size_of;

    /// Build a valid in-memory blueprint file for testing.
    fn create_test_blueprint(
        infoset_count: u64,
        max_actions_k: u8,
        fmph_level_count: u32,
        fmph_max_level_size: u64,
        tt_num_entries: u64,
        tt_action_size: u32,
    ) -> Vec<u8> {
        let mut buf = Vec::new();

        // FileHeader
        let fh = FileHeader {
            magic: *MAGIC,
            version: 1,
            variant_id: 0,
            infoset_count,
            max_actions_k,
            _padding: [0; 7],
        };
        buf.write_all(bytemuck::bytes_of(&fh)).unwrap();

        // FmphHeader
        let fmp_hdr = FmphHeader {
            num_keys: infoset_count,
            seed: 42,
            max_level_size: fmph_max_level_size,
            level_count: fmph_level_count,
            _padding: [0; 4],
        };
        buf.write_all(bytemuck::bytes_of(&fmp_hdr)).unwrap();

        // Fmph data (level_count * max_level_size u32 values)
        let fmph_data_len = fmph_level_count as usize * fmph_max_level_size as usize;
        for i in 0..fmph_data_len {
            let val: u32 = i as u32;
            buf.write_all(&val.to_le_bytes()).unwrap();
        }

        // TranslationTableHeader
        let tth = TranslationTableHeader {
            num_entries: tt_num_entries,
            action_size: tt_action_size,
            _padding: [0; 4],
        };
        buf.write_all(bytemuck::bytes_of(&tth)).unwrap();

        // Translation table entries
        for i in 0..(tt_num_entries * tt_action_size as u64) {
            buf.push((i % 256) as u8);
        }

        // CDF data
        let cdf_len = infoset_count as usize * max_actions_k as usize;
        for i in 0..cdf_len {
            buf.push((i % 256) as u8);
        }

        buf
    }

    #[test]
    fn test_open_valid_blueprint() {
        let data = create_test_blueprint(10, 3, 2, 100, 50, 1);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();

        let reader = MmapReader::new(tmp.path()).unwrap();
        assert_eq!(reader.file_header().infoset_count, 10);
        assert_eq!(reader.file_header().max_actions_k, 3);
        assert_eq!(reader.fmph_header().num_keys, 10);
        assert_eq!(reader.fmph_header().level_count, 2);
        assert_eq!(reader.fmph_header().max_level_size, 100);
        assert_eq!(reader.translation_table_header().num_entries, 50);
        assert_eq!(reader.translation_table_header().action_size, 1);

        // Check slice lengths
        assert_eq!(reader.fmph_data().len(), 2 * 100 * 4);
        assert_eq!(reader.translation_table_data().len(), 50 * 1);
        assert_eq!(reader.cdf_data().len(), 10 * 3);
    }

    #[test]
    fn test_fmph_data_content() {
        let data = create_test_blueprint(5, 2, 1, 10, 0, 0);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();

        let reader = MmapReader::new(tmp.path()).unwrap();
        let fmph_bytes = reader.fmph_data();
        assert_eq!(fmph_bytes.len(), 1 * 10 * 4); // 40 bytes

        // Verify first u32 == 0, second == 1
        let first_u32 = u32::from_le_bytes(fmph_bytes[0..4].try_into().unwrap());
        let second_u32 = u32::from_le_bytes(fmph_bytes[4..8].try_into().unwrap());
        assert_eq!(first_u32, 0);
        assert_eq!(second_u32, 1);
    }

    #[test]
    fn test_cdf_data_content() {
        let data = create_test_blueprint(3, 2, 0, 0, 0, 0); // no fmph data, no tt
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();

        let reader = MmapReader::new(tmp.path()).unwrap();
        let cdf = reader.cdf_data();
        assert_eq!(cdf.len(), 6);
        // CDF filled with i % 256
        for (i, &byte) in cdf.iter().enumerate() {
            assert_eq!(byte, (i % 256) as u8);
        }
    }

    #[test]
    fn test_translation_table_content() {
        let data = create_test_blueprint(1, 1, 0, 0, 4, 2);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();

        let reader = MmapReader::new(tmp.path()).unwrap();
        let tt = reader.translation_table_data();
        assert_eq!(tt.len(), 8);
        for (i, &byte) in tt.iter().enumerate() {
            assert_eq!(byte, (i % 256) as u8);
        }
    }

    #[test]
    fn test_file_too_small() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &[0u8; 10]).unwrap(); // too small for header
        let err = MmapReader::new(tmp.path()).unwrap_err();
        assert!(matches!(err, MmapError::FileTooSmall));
    }

    #[test]
    fn test_invalid_magic() {
        let mut data = create_test_blueprint(1, 1, 0, 0, 0, 0);
        data[0] = 0xFF; // corrupt magic
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let err = MmapReader::new(tmp.path()).unwrap_err();
        assert!(matches!(err, MmapError::InvalidMagic { .. }));
    }

    #[test]
    fn test_unsupported_version() {
        let mut data = create_test_blueprint(1, 1, 0, 0, 0, 0);
        // FileHeader.version is at offset 8 (after 8-byte magic)
        data[8] = 99; // change version to 99
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let err = MmapReader::new(tmp.path()).unwrap_err();
        assert!(matches!(err, MmapError::UnsupportedVersion(99)));
    }

    #[test]
    fn test_truncated_cdf_detected() {
        let data = create_test_blueprint(100, 5, 0, 0, 0, 0);
        // Cut off half the CDF data
        let truncated_len = data.len() - 250;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data[..truncated_len]).unwrap();
        let err = MmapReader::new(tmp.path()).unwrap_err();
        assert!(matches!(err, MmapError::InvalidOffset(..)));
    }

    #[test]
    fn test_zero_length_sections() {
        let data = create_test_blueprint(0, 0, 0, 0, 0, 0);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let reader = MmapReader::new(tmp.path()).unwrap();
        assert_eq!(reader.fmph_data().len(), 0);
        assert_eq!(reader.translation_table_data().len(), 0);
        assert_eq!(reader.cdf_data().len(), 0);
    }

    #[test]
    fn test_header_pointers_stable() {
        let data = create_test_blueprint(7, 4, 1, 50, 10, 2);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();

        let reader = MmapReader::new(tmp.path()).unwrap();
        let fh1 = reader.file_header() as *const FileHeader;
        let fh2 = reader.file_header() as *const FileHeader;
        assert_eq!(fh1, fh2, "file_header() should return same pointer");

        // Check data integrity after multiple calls
        assert_eq!(reader.file_header().infoset_count, 7);
        assert_eq!(reader.file_header().max_actions_k, 4);
        assert_eq!(reader.fmph_header().level_count, 1);
        assert_eq!(reader.fmph_header().max_level_size, 50);
        assert_eq!(reader.translation_table_header().num_entries, 10);
        assert_eq!(reader.translation_table_header().action_size, 2);
    }

    #[test]
    fn test_file_not_found() {
        let err = MmapReader::new("/nonexistent/path/to/blueprint.bin").unwrap_err();
        assert!(matches!(err, MmapError::Io(..)));
    }

    #[test]
    fn test_large_blueprint() {
        let data = create_test_blueprint(10000, 5, 4, 1000, 5000, 1);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();

        let reader = MmapReader::new(tmp.path()).unwrap();
        assert_eq!(reader.file_header().infoset_count, 10000);
        assert_eq!(reader.fmph_data().len(), 4 * 1000 * 4);
        assert_eq!(reader.translation_table_data().len(), 5000);
        assert_eq!(reader.cdf_data().len(), 10000 * 5);
    }
}
