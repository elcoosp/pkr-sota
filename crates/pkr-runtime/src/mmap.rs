use bytemuck;
use memmap2::Mmap;
use pkr_export::header::{FileHeader, FORMAT_VERSION_V2, HASH_ALGO_FNV1A64_INFOSET};
use std::fs::File;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MmapError {
    #[error("failed to open blueprint file: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid magic bytes: expected {expected:?}, got {actual:?}")]
    InvalidMagic { expected: [u8; 8], actual: [u8; 8] },
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u32),
    #[error(
        "blueprint was built with hash_algo={0} — expected FNV-1a 64-bit ({1}) (silent-uniform \
         bug guard; re-run pkr-export)"
    )]
    InvalidHashAlgo(u8, u8),
    #[error("file too small for header")]
    FileTooSmall,
    #[error("invalid section offset: {0}")]
    InvalidOffset(&'static str),
}

const MAGIC: &[u8; 8] = b"PKRSOTA1";

#[derive(Debug)]
pub struct MmapReader {
    mmap: Mmap,
    file_header: FileHeader,
    offset_keys: usize,
    num_keys: usize,
    offset_cdf: usize,
    len_cdf: usize,
}

unsafe impl Send for MmapReader {}
unsafe impl Sync for MmapReader {}

impl MmapReader {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, MmapError> {
        let file = File::open(path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        if mmap.len() < std::mem::size_of::<FileHeader>() {
            return Err(MmapError::FileTooSmall);
        }
        let file_header: FileHeader =
            *bytemuck::from_bytes(&mmap[..std::mem::size_of::<FileHeader>()]);

        if &file_header.magic != MAGIC {
            return Err(MmapError::InvalidMagic {
                expected: *MAGIC,
                actual: file_header.magic,
            });
        }
        // Accept v2 (no anchors) and v3 (anchors section present).
        if file_header.version < FORMAT_VERSION_V2 {
            return Err(MmapError::UnsupportedVersion(file_header.version));
        }
        if file_header.hash_algo != HASH_ALGO_FNV1A64_INFOSET {
            return Err(MmapError::InvalidHashAlgo(
                file_header.hash_algo,
                HASH_ALGO_FNV1A64_INFOSET,
            ));
        }

        // After FileHeader, v3 files have a 48-byte AnchorsSection, then
        // two u32 (key_count, cdf_bytes_len). v2 files have no anchors.
        let after_file_header = std::mem::size_of::<FileHeader>();
        let anchors_size = if file_header.version >= 3 { 48 } else { 0 };
        let after_header = after_file_header + anchors_size;
        if mmap.len() < after_header + 8 {
            return Err(MmapError::FileTooSmall);
        }
        let key_count =
            u32::from_le_bytes(mmap[after_header..after_header + 4].try_into().unwrap()) as usize;
        let cdf_bytes_len =
            u32::from_le_bytes(mmap[after_header + 4..after_header + 8].try_into().unwrap())
                as usize;

        let offset_keys = after_header + 8;
        let keys_bytes = key_count * 8;
        let offset_cdf = offset_keys + keys_bytes;

        if mmap.len() < offset_cdf + cdf_bytes_len {
            return Err(MmapError::InvalidOffset("data truncated"));
        }

        Ok(MmapReader {
            mmap,
            file_header,
            offset_keys,
            num_keys: key_count,
            offset_cdf,
            len_cdf: cdf_bytes_len,
        })
    }

    #[inline]
    pub fn file_header(&self) -> &FileHeader {
        &self.file_header
    }

    /// T2.1: per-street bet-size anchors. v3+ files store them after the
    /// header; v2 files return the compile-time default.
    #[inline]
    pub fn anchors(&self) -> [[f32; 3]; 4] {
        if self.file_header.version >= 3 {
            let base = std::mem::size_of::<FileHeader>();
            let raw = &self.mmap[base..base + 48];
            let s: &pkr_export::header::AnchorsSection =
                bytemuck::from_bytes(raw);
            s.anchors
        } else {
            pkr_export::header::ANCHORS
        }
    }

    #[inline]
    pub fn keys_data(&self) -> &[u8] {
        &self.mmap[self.offset_keys..self.offset_keys + self.num_keys * 8]
    }

    #[inline]
    pub fn cdf_data(&self) -> &[u8] {
        &self.mmap[self.offset_cdf..self.offset_cdf + self.len_cdf]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_test_blueprint(infoset_count: u64, max_actions_k: u8) -> Vec<u8> {
        let mut buf = Vec::new();
        let fh = FileHeader {
            magic: *MAGIC,
            version: FORMAT_VERSION_V2,
            variant_id: 0,
            infoset_count,
            max_actions_k,
            hash_algo: HASH_ALGO_FNV1A64_INFOSET,
            _padding: [0; 6],
        };
        buf.write_all(bytemuck::bytes_of(&fh)).unwrap();
        let kc = infoset_count as u32;
        let cdf_len = infoset_count as usize * max_actions_k as usize;
        buf.write_all(&kc.to_le_bytes()).unwrap();
        buf.write_all(&(cdf_len as u32).to_le_bytes()).unwrap();
        // Write dummy keys and CDF
        for _ in 0..infoset_count {
            buf.write_all(&[0u8; 8]).unwrap();
        }
        buf.extend(std::iter::repeat_n(0u8, cdf_len));
        buf
    }

    #[test]
    fn test_open_valid_blueprint() {
        let data = create_test_blueprint(10, 3);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let reader = MmapReader::new(tmp.path()).unwrap();
        assert_eq!(reader.file_header().infoset_count, 10);
        assert_eq!(reader.keys_data().len(), 80);
        assert_eq!(reader.cdf_data().len(), 30);
    }

    #[test]
    fn test_file_too_small() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), [0u8; 10]).unwrap();
        assert!(MmapReader::new(tmp.path()).is_err());
    }

    #[test]
    fn test_rejects_legacy_hash_algo() {
        // Simulate a blueprint built with the old DefaultHasher (hash_algo=1)
        let mut data = create_test_blueprint(10, 3);
        // patch the hash_algo byte at offset 28 (after magic[8] + version[4] + variant_id[4] +
        // infoset_count[8] + max_actions_k[1] = 25; hash_algo is at 25)
        data[25] = 1;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let result = MmapReader::new(tmp.path());
        assert!(result.is_err());
        match result.unwrap_err() {
            MmapError::InvalidHashAlgo(1, _) => {}
            other => panic!("expected InvalidHashAlgo, got {:?}", other),
        }
    }
}
