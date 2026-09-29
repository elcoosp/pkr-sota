use bytemuck;
use memmap2::Mmap;
#[cfg(test)]
use pkr_export::header::FORMAT_VERSION_V3;
use pkr_export::header::{
    FileHeader, FORMAT_VERSION_V2, FORMAT_VERSION_V4, HASH_ALGO_FNV1A64_INFOSET,
};
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
    #[error("blueprint abstraction mismatch: {0}")]
    FingerprintMismatch(String),
}

const MAGIC: &[u8; 8] = b"PKRSOTA1";

/// Memory-mapped reader for a blueprint file.
///
/// # Memory-mapping contract
///
/// The file is mapped with `memmap2::Mmap` and read directly on every
/// lookup — no copy is held in memory. **The file must not be modified
/// or truncated while this reader (or any `SolverHandle` derived from
/// it) is alive.** Modifying the underlying file is undefined behavior:
/// the kernel may serve stale bytes for pages already mapped, may
/// SIGBUS on a page that no longer exists, or may see a torn write on
/// a page that's mid-update. This is inherited from `memmap2` and
/// applies to every use of `MmapReader`.
///
/// The intended workflow is: write the blueprint once, then open a
/// reader and never touch the file for the lifetime of the process.
/// A host that needs to swap blueprints should drop the reader and
/// handle, replace the file atomically (rename a temp over the path),
/// and open a fresh reader.
///
/// The export path (`pkr-export::writer`) writes to a temp file and
/// renames, so a running reader on the old path keeps a consistent
/// view of the old bytes until it's dropped.
#[derive(Debug)]
pub struct MmapReader {
    mmap: Mmap,
    file_header: FileHeader,
    offset_keys: usize,
    num_keys: usize,
    offset_cdf: usize,
    len_cdf: usize,
    fingerprint: Option<pkr_core::abstraction::AbstractionFingerprint>,
    /// P3-b: parsed FMph tail, if the writer emitted one.
    fmph: Option<FmphView>,
}

/// Parsed FMph section (P3-b). Only present in files written after P3-b.
#[derive(Debug, Clone)]
pub struct FmphView {
    pub seed1: u64,
    pub seed2: u64,
    pub bucket_count: usize,
    pub num_keys: usize,
    pub displacements: Vec<u32>,
}

unsafe impl Send for MmapReader {}
unsafe impl Sync for MmapReader {}

impl MmapReader {
    /// P3-b: parsed FMph tail, if this blueprint was written by a
    /// post-P3-b exporter. `None` for older files (search fallback used).
    pub fn fmph(&self) -> Option<&FmphView> {
        self.fmph.as_ref()
    }

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
        // Accept v2 (no anchors), v3 (anchors), v4 (anchors + fingerprint).
        // Anything newer is rejected -- parsing it as v4 would silently
        // misinterpret the layout (audit B6).
        if file_header.version < FORMAT_VERSION_V2 || file_header.version > FORMAT_VERSION_V4 {
            return Err(MmapError::UnsupportedVersion(file_header.version));
        }
        if file_header.hash_algo != HASH_ALGO_FNV1A64_INFOSET {
            return Err(MmapError::InvalidHashAlgo(
                file_header.hash_algo,
                HASH_ALGO_FNV1A64_INFOSET,
            ));
        }

        // Layout by version (see pkr-export/src/writer.rs):
        //   v2: [FH:32][key_count:4][cdf_size:4][keys][cdf]
        //   v3: [FH:32][Anchors:48][key_count:4][cdf_size:4][keys][cdf]
        //   v4: [FH:32][Anchors:48][Fingerprint:40][key_count:4][cdf_size:4][keys][cdf]
        let after_file_header = std::mem::size_of::<FileHeader>();
        let anchors_size = if file_header.version >= 3 { 48 } else { 0 };
        let fp_size = if file_header.version >= FORMAT_VERSION_V4 {
            std::mem::size_of::<pkr_core::abstraction::AbstractionFingerprint>()
        } else {
            0
        };
        let after_header = after_file_header + anchors_size + fp_size;

        if mmap.len() < after_header + 8 {
            return Err(MmapError::FileTooSmall);
        }

        let fingerprint = if fp_size > 0 {
            let base = after_file_header + anchors_size;
            let raw = &mmap[base..base + fp_size];
            Some(bytemuck::pod_read_unaligned::<
                pkr_core::abstraction::AbstractionFingerprint,
            >(raw))
        } else {
            // v2/v3: no fingerprint. Emit a one-time warning.
            use std::sync::OnceLock;
            static WARNED: OnceLock<()> = OnceLock::new();
            WARNED.get_or_init(|| {
                eprintln!(
                    "WARNING: blueprint is v{} (no abstraction fingerprint). \
                     Upgrade with the current pkr-trainer to embed the \
                     semantic-config fingerprint in v4 blueprints.",
                    file_header.version,
                );
            });
            None
        };

        let key_count =
            u32::from_le_bytes(mmap[after_header..after_header + 4].try_into().unwrap()) as usize;
        let cdf_bytes_len =
            u32::from_le_bytes(mmap[after_header + 4..after_header + 8].try_into().unwrap())
                as usize;

        let max_k = file_header.max_actions_k as usize;
        if max_k == 0 || max_k > 16 {
            return Err(MmapError::InvalidOffset("max_actions_k out of range"));
        }
        let keys_bytes = key_count
            .checked_mul(8)
            .ok_or(MmapError::InvalidOffset("key_count overflow"))?;
        if Some(cdf_bytes_len) != key_count.checked_mul(max_k) {
            return Err(MmapError::InvalidOffset(
                "cdf size != key_count * max_actions_k",
            ));
        }
        let offset_keys = after_header + 8;
        let offset_cdf = offset_keys + keys_bytes;

        if mmap.len() < offset_cdf + cdf_bytes_len {
            return Err(MmapError::InvalidOffset("data truncated"));
        }

        // P3-b: try to parse an optional FMph tail (after the CDF).
        // Layout: [FmphHeader:40][displacements: u32 * bucket_count].
        // Absent in pre-P3-b files; runtime falls back to branchless search.
        let fmph = {
            use pkr_export::header::FmphHeader;
            let hdr_size = std::mem::size_of::<FmphHeader>();
            let tail_off = offset_cdf + cdf_bytes_len;
            if mmap.len() >= tail_off + hdr_size {
                let hdr_bytes = &mmap[tail_off..tail_off + hdr_size];
                let hdr: FmphHeader = bytemuck::pod_read_unaligned(hdr_bytes);
                // Section-present flag: level_count == 1 AND num_keys matches.
                if hdr.level_count == 1 && hdr.num_keys == key_count as u64 {
                    let disp_off = tail_off + hdr_size;
                    let disp_count = hdr.max_level_size as usize;
                    let disp_bytes = disp_count
                        .checked_mul(4)
                        .ok_or(MmapError::InvalidOffset("fmPH size overflow"))?;
                    if mmap.len() >= disp_off + disp_bytes {
                        let mut displacements = Vec::with_capacity(disp_count);
                        for i in 0..disp_count {
                            let b = &mmap[disp_off + i * 4..disp_off + i * 4 + 4];
                            displacements.push(u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                        }
                        Some(FmphView {
                            seed1: hdr.seed1,
                            seed2: hdr.seed2,
                            bucket_count: disp_count,
                            num_keys: key_count as usize,
                            displacements,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        Ok(MmapReader {
            mmap,
            file_header,
            offset_keys,
            num_keys: key_count,
            offset_cdf,
            len_cdf: cdf_bytes_len,
            fingerprint,
            fmph,
        })
    }

    /// F2c: enforcement helper. Callers that want to refuse a
    /// blueprint whose abstraction doesn't match their current config
    /// should use this after `new`. Returns `Ok(())` if the blueprint
    /// has no fingerprint (v3) or if it matches `current`.
    pub fn check_fingerprint(
        &self,
        current: &pkr_core::abstraction::AbstractionFingerprint,
    ) -> Result<(), MmapError> {
        if let Some(stored) = self.fingerprint {
            if stored != *current {
                return Err(MmapError::FingerprintMismatch(
                    stored.describe_mismatch(current),
                ));
            }
        }
        Ok(())
    }

    /// F2c: expose the stored fingerprint for diagnostic logging.
    /// `None` for v2/v3 blueprints.
    pub fn fingerprint(&self) -> Option<pkr_core::abstraction::AbstractionFingerprint> {
        self.fingerprint
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
            let s: &pkr_export::header::AnchorsSection = bytemuck::from_bytes(raw);
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

    // ------------------------------------------------------------------
    // B6 regression tests (audit: "version > 4 accepted as v4; cdf length
    // not validated; max_actions_k range unchecked").
    // ------------------------------------------------------------------

    fn create_test_blueprint_with(version: u32, k: u8, cdf_len_override: Option<u32>) -> Vec<u8> {
        let mut buf = Vec::new();
        let fh = FileHeader {
            magic: *MAGIC,
            version,
            variant_id: 0,
            infoset_count: 10,
            max_actions_k: k,
            hash_algo: HASH_ALGO_FNV1A64_INFOSET,
            _padding: [0; 6],
        };
        buf.write_all(bytemuck::bytes_of(&fh)).unwrap();
        if version >= FORMAT_VERSION_V3 {
            buf.extend(std::iter::repeat_n(0u8, 48));
        }
        if version >= FORMAT_VERSION_V4 {
            buf.extend(std::iter::repeat_n(0u8, 40));
        }
        let kc: u32 = 10;
        let cdf_len: u32 = cdf_len_override.unwrap_or(kc * k as u32);
        buf.write_all(&kc.to_le_bytes()).unwrap();
        buf.write_all(&cdf_len.to_le_bytes()).unwrap();
        for _ in 0..kc {
            buf.write_all(&[0u8; 8]).unwrap();
        }
        buf.extend(std::iter::repeat_n(0u8, cdf_len as usize));
        buf
    }

    #[test]
    fn b6_rejects_version_above_v4() {
        let data = create_test_blueprint_with(FORMAT_VERSION_V4 + 1, 3, None);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let r = MmapReader::new(tmp.path());
        assert!(r.is_err(), "version above v4 must be rejected");
    }

    #[test]
    fn b6_rejects_max_k_zero() {
        let data = create_test_blueprint_with(FORMAT_VERSION_V2, 0, None);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let r = MmapReader::new(tmp.path());
        assert!(r.is_err(), "max_actions_k=0 must be rejected");
    }

    #[test]
    fn b6_rejects_cdf_len_mismatch() {
        // k=3, keys=10 => correct cdf_len is 30; advertise 29.
        let data = create_test_blueprint_with(FORMAT_VERSION_V2, 3, Some(29));
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let r = MmapReader::new(tmp.path());
        assert!(r.is_err(), "cdf_len != key_count * k must be rejected");
    }
}
