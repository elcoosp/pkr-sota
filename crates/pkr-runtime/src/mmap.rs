use std::fs::File;
use std::path::Path;
use bytemuck;
use memmap2::Mmap;
use pkr_export::header::{FileHeader, FmphHeader, TranslationTableHeader};
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
    offset_fmph_header: usize,
    offset_fmph_data: usize,
    len_fmph_data: usize,
    offset_translation_header: usize,
    offset_translation_data: usize,
    len_translation_data: usize,
    offset_keys: usize,     // key verification table (infoset_count * 8 bytes)
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
        if file_header.version != SUPPORTED_VERSION {
            return Err(MmapError::UnsupportedVersion(file_header.version));
        }

        let offset_fmph_header = std::mem::size_of::<FileHeader>();
        let offset_fmph_data = offset_fmph_header + std::mem::size_of::<FmphHeader>();

        let fmph_header_end = offset_fmph_header + std::mem::size_of::<FmphHeader>();
        if mmap.len() < fmph_header_end {
            return Err(MmapError::FileTooSmall);
        }
        let fmph_header: &FmphHeader =
            bytemuck::from_bytes(&mmap[offset_fmph_header..fmph_header_end]);

        let len_fmph_data =
            fmph_header.level_count as u64 * fmph_header.max_level_size as u64 * 4;
        let len_fmph_data = len_fmph_data as usize;

        let offset_translation_header = offset_fmph_data + len_fmph_data;
        let offset_translation_data =
            offset_translation_header + std::mem::size_of::<TranslationTableHeader>();

        let tt_header_end =
            offset_translation_header + std::mem::size_of::<TranslationTableHeader>();
        if mmap.len() < tt_header_end {
            return Err(MmapError::FileTooSmall);
        }
        let tt_header: &TranslationTableHeader =
            bytemuck::from_bytes(&mmap[offset_translation_header..tt_header_end]);

        let len_translation_data =
            tt_header.num_entries as u64 * tt_header.action_size as u64;
        let len_translation_data = len_translation_data as usize;

        let offset_keys = offset_translation_data + len_translation_data;
        let keys_len = file_header.infoset_count as usize * 8;
        let offset_cdf = offset_keys + keys_len;
        let len_cdf =
            file_header.infoset_count as usize * file_header.max_actions_k as usize;

        let total_required = offset_cdf + len_cdf;
        if mmap.len() < total_required {
            return Err(MmapError::InvalidOffset(
                "data extends past end of file",
            ));
        }

        Ok(MmapReader {
            mmap,
            file_header,
            offset_fmph_header,
            offset_fmph_data,
            len_fmph_data,
            offset_translation_header,
            offset_translation_data,
            len_translation_data,
            offset_keys,
            offset_cdf,
            len_cdf,
        })
    }

    #[inline]
    pub fn file_header(&self) -> &FileHeader {
        &self.file_header
    }

    #[inline]
    pub fn fmph_header(&self) -> &FmphHeader {
        let slice = &self.mmap
            [self.offset_fmph_header..self.offset_fmph_header + std::mem::size_of::<FmphHeader>()];
        bytemuck::from_bytes(slice)
    }

    #[inline]
    pub fn fmph_data(&self) -> &[u8] {
        &self.mmap[self.offset_fmph_data..self.offset_fmph_data + self.len_fmph_data]
    }

    #[inline]
    pub fn keys_data(&self) -> &[u8] {
        let keys_len = self.file_header.infoset_count as usize * 8;
        &self.mmap[self.offset_keys..self.offset_keys + keys_len]
    }

    #[inline]
    pub fn translation_table_header(&self) -> &TranslationTableHeader {
        let slice = &self.mmap[self.offset_translation_header
            ..self.offset_translation_header + std::mem::size_of::<TranslationTableHeader>()];
        bytemuck::from_bytes(slice)
    }

    #[inline]
    pub fn translation_table_data(&self) -> &[u8] {
        &self.mmap
            [self.offset_translation_data..self.offset_translation_data + self.len_translation_data]
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

    fn create_test_blueprint(
        infoset_count: u64,
        max_actions_k: u8,
        fmph_level_count: u32,
        fmph_max_level_size: u64,
        tt_num_entries: u64,
        tt_action_size: u32,
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        let fh = FileHeader {
            magic: *MAGIC,
            version: 1,
            variant_id: 0,
            infoset_count,
            max_actions_k,
            _padding: [0; 7],
        };
        buf.write_all(bytemuck::bytes_of(&fh)).unwrap();
        let fmp_hdr = FmphHeader {
            num_keys: infoset_count,
            seed1: 42,
            seed2: 0,
            max_level_size: fmph_max_level_size,
            level_count: fmph_level_count,
            _padding: [0; 4],
        };
        buf.write_all(bytemuck::bytes_of(&fmp_hdr)).unwrap();
        let fmph_data_len = fmph_level_count as usize * fmph_max_level_size as usize;
        for i in 0..fmph_data_len {
            let val: u32 = i as u32;
            buf.write_all(&val.to_le_bytes()).unwrap();
        }
        let tth = TranslationTableHeader {
            num_entries: tt_num_entries,
            action_size: tt_action_size,
            _padding: [0; 4],
        };
        buf.write_all(bytemuck::bytes_of(&tth)).unwrap();
        for i in 0..(tt_num_entries * tt_action_size as u64) {
            buf.push((i % 256) as u8);
        }
        // Key table (infoset_count * 8 bytes of zeros)
        for _ in 0..(infoset_count as usize * 8) {
            buf.push(0);
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
        assert_eq!(reader.keys_data().len(), 80);
    }
}
