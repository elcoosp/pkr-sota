use std::fs::File;
use std::path::Path;
use bytemuck;
use memmap2::Mmap;
use pkr_export::header::FileHeader;
use thiserror::Error;

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

const MAGIC: &[u8; 8] = b"PKRSOTA1";
const SUPPORTED_VERSION: u32 = 1;

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
            return Err(MmapError::InvalidMagic { expected: *MAGIC, actual: file_header.magic });
        }
        if file_header.version != SUPPORTED_VERSION {
            return Err(MmapError::UnsupportedVersion(file_header.version));
        }

        // After FileHeader, we have two u32: key_count and cdf_bytes_len
        let after_header = std::mem::size_of::<FileHeader>();
        if mmap.len() < after_header + 8 {
            return Err(MmapError::FileTooSmall);
        }
        let key_count = u32::from_le_bytes(mmap[after_header..after_header+4].try_into().unwrap()) as usize;
        let cdf_bytes_len = u32::from_le_bytes(mmap[after_header+4..after_header+8].try_into().unwrap()) as usize;

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
            magic: *MAGIC, version: 1, variant_id: 0,
            infoset_count, max_actions_k,
            _padding: [0; 7],
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
        for _ in 0..cdf_len {
            buf.push(0u8);
        }
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
        std::fs::write(tmp.path(), &[0u8; 10]).unwrap();
        assert!(MmapReader::new(tmp.path()).is_err());
    }
}
