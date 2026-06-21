use bytemuck::{Pod, Zeroable};

/// Magic number for the blueprint file format: "PKRSOTA1"
#[allow(dead_code)]
const MAGIC: &[u8; 8] = b"PKRSOTA1";

/// Top-level header of the blueprint file.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct FileHeader {
    /// Magic bytes to identify the file format.
    pub magic: [u8; 8],
    /// Format version number.
    pub version: u32,
    /// Identifier for the poker variant.
    pub variant_id: u32,
    /// Total number of information sets in the file.
    pub infoset_count: u64,
    /// Maximum number of legal actions at any decision point (K).
    pub max_actions_k: u8,
    /// Explicit padding to ensure no implicit padding and alignment to 8 bytes.
    pub _padding: [u8; 7],
}

/// Header for the Fmph (Fingerprint Minimal Perfect Hash) section.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct FmphHeader {
    /// Number of keys (information sets) indexed by the Fmph.
    pub num_keys: u64,
    /// Random seed used for hash function construction.
    pub seed: u64,
    /// Maximum size of a single level in the MPH.
    pub max_level_size: u64,
    /// Number of levels in the MPH.
    pub level_count: u32,
    /// Padding to 8‑byte alignment.
    pub _padding: [u8; 4],
}

/// Header for the translation table that maps abstract actions to concrete actions.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TranslationTableHeader {
    /// Number of entries in the translation table.
    pub num_entries: u64,
    /// Size of each entry in bytes (e.g., 1 if actions fit in one byte).
    pub action_size: u32,
    /// Padding to 8‑byte alignment.
    pub _padding: [u8; 4],
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, size_of};

    // ---------- existing size/alignment tests ----------
    #[test]
    fn test_file_header_sizes() {
        assert_eq!(size_of::<FileHeader>(), 32, "FileHeader must be 32 bytes");
        assert_eq!(
            align_of::<FileHeader>(),
            8,
            "FileHeader must be 8-byte aligned"
        );
    }

    #[test]
    fn test_fmph_header_sizes() {
        assert_eq!(size_of::<FmphHeader>(), 32, "FmphHeader must be 32 bytes");
        assert_eq!(
            align_of::<FmphHeader>(),
            8,
            "FmphHeader must be 8-byte aligned"
        );
    }

    #[test]
    fn test_translation_table_header_sizes() {
        assert_eq!(
            size_of::<TranslationTableHeader>(),
            16,
            "TranslationTableHeader must be 16 bytes"
        );
        assert_eq!(
            align_of::<TranslationTableHeader>(),
            8,
            "TranslationTableHeader must be 8-byte aligned"
        );
    }

    #[test]
    fn test_file_header_magic() {
        let hdr = FileHeader {
            magic: *MAGIC,
            version: 1,
            variant_id: 0,
            infoset_count: 0,
            max_actions_k: 0,
            _padding: [0; 7],
        };
        assert_eq!(&hdr.magic, b"PKRSOTA1");
    }

    #[test]
    fn test_file_header_is_pod() {
        fn is_pod<T: bytemuck::Pod>() {}
        is_pod::<FileHeader>();
        is_pod::<FmphHeader>();
        is_pod::<TranslationTableHeader>();
    }

    #[test]
    fn test_zeroable() {
        let fh: FileHeader = Zeroable::zeroed();
        assert_eq!(fh.magic, [0u8; 8]);
        assert_eq!(fh.version, 0);
        assert_eq!(fh.variant_id, 0);
        assert_eq!(fh.infoset_count, 0);
        assert_eq!(fh.max_actions_k, 0);
    }

    // ---------- new tests: round-trip via bytes ----------
    #[test]
    fn roundtrip_file_header() {
        let original = FileHeader {
            magic: *b"PKRSOTA1",
            version: 2,
            variant_id: 42,
            infoset_count: 1_000_000,
            max_actions_k: 5,
            _padding: [0; 7],
        };
        let bytes = bytemuck::bytes_of(&original);
        // cast back
        let recovered: &FileHeader = bytemuck::from_bytes(bytes);
        assert_eq!(original.magic, recovered.magic);
        assert_eq!(original.version, recovered.version);
        assert_eq!(original.variant_id, recovered.variant_id);
        assert_eq!(original.infoset_count, recovered.infoset_count);
        assert_eq!(original.max_actions_k, recovered.max_actions_k);
    }

    #[test]
    fn roundtrip_fmph_header() {
        let original = FmphHeader {
            num_keys: 500_000,
            seed: 0xdead_beef_cafe_babe,
            max_level_size: 1024,
            level_count: 3,
            _padding: [0; 4],
        };
        let bytes = bytemuck::bytes_of(&original);
        let recovered: &FmphHeader = bytemuck::from_bytes(bytes);
        assert_eq!(original.num_keys, recovered.num_keys);
        assert_eq!(original.seed, recovered.seed);
        assert_eq!(original.max_level_size, recovered.max_level_size);
        assert_eq!(original.level_count, recovered.level_count);
    }

    #[test]
    fn roundtrip_translation_table_header() {
        let original = TranslationTableHeader {
            num_entries: 123456,
            action_size: 1,
            _padding: [0; 4],
        };
        let bytes = bytemuck::bytes_of(&original);
        let recovered: &TranslationTableHeader = bytemuck::from_bytes(bytes);
        assert_eq!(original.num_entries, recovered.num_entries);
        assert_eq!(original.action_size, recovered.action_size);
    }

    // ---------- new tests: contiguous zero-copy casting ----------
    #[test]
    fn cast_slice_file_headers() {
        let mut headers = vec![
            FileHeader {
                magic: *b"PKRSOTA1",
                version: 1,
                variant_id: 0,
                infoset_count: 10,
                max_actions_k: 3,
                _padding: [0; 7],
            },
            FileHeader {
                magic: *b"PKRSOTA1",
                version: 1,
                variant_id: 1,
                infoset_count: 20,
                max_actions_k: 4,
                _padding: [0; 7],
            },
        ];
        let bytes: &[u8] = bytemuck::cast_slice(&headers);
        assert_eq!(bytes.len(), 2 * size_of::<FileHeader>());
        // cast back
        let recovered: &[FileHeader] = bytemuck::cast_slice(bytes);
        assert_eq!(recovered.len(), 2);
        assert_eq!(recovered[0].variant_id, 0);
        assert_eq!(recovered[1].variant_id, 1);
    }

    #[test]
    fn cast_slice_fmph_headers() {
        let headers = vec![
            FmphHeader {
                num_keys: 100,
                seed: 42,
                max_level_size: 256,
                level_count: 2,
                _padding: [0; 4],
            },
            FmphHeader {
                num_keys: 200,
                seed: 43,
                max_level_size: 512,
                level_count: 4,
                _padding: [0; 4],
            },
        ];
        let bytes: &[u8] = bytemuck::cast_slice(&headers);
        let recovered: &[FmphHeader] = bytemuck::cast_slice(bytes);
        assert_eq!(recovered[0].num_keys, 100);
        assert_eq!(recovered[1].num_keys, 200);
    }

    #[test]
    fn cast_slice_translation_table_headers() {
        let headers = vec![
            TranslationTableHeader {
                num_entries: 50,
                action_size: 1,
                _padding: [0; 4],
            },
            TranslationTableHeader {
                num_entries: 100,
                action_size: 2,
                _padding: [0; 4],
            },
        ];
        let bytes: &[u8] = bytemuck::cast_slice(&headers);
        let recovered: &[TranslationTableHeader] = bytemuck::cast_slice(bytes);
        assert_eq!(recovered[0].action_size, 1);
        assert_eq!(recovered[1].action_size, 2);
    }

    // ---------- new tests: zeroed structs have zero padding ----------
    #[test]
    fn zeroed_file_header_padding() {
        let hdr: FileHeader = Zeroable::zeroed();
        assert_eq!(hdr._padding, [0u8; 7]);
    }

    #[test]
    fn zeroed_fmph_header_padding() {
        let hdr: FmphHeader = Zeroable::zeroed();
        assert_eq!(hdr._padding, [0u8; 4]);
    }

    #[test]
    fn zeroed_translation_header_padding() {
        let hdr: TranslationTableHeader = Zeroable::zeroed();
        assert_eq!(hdr._padding, [0u8; 4]);
    }

    // ---------- new test: verify alignment of arrays ----------
    #[test]
    fn alignment_in_array() {
        let arr = [FileHeader::zeroed(); 4];
        let base_addr = &arr[0] as *const _ as usize;
        for i in 1..4 {
            let addr = &arr[i] as *const _ as usize;
            assert_eq!((addr - base_addr) % align_of::<FileHeader>(), 0);
        }
    }

    #[test]
    fn alignment_of_fmph_array() {
        let arr = [FmphHeader::zeroed(); 4];
        let base_addr = &arr[0] as *const _ as usize;
        for i in 1..4 {
            let addr = &arr[i] as *const _ as usize;
            assert_eq!((addr - base_addr) % align_of::<FmphHeader>(), 0);
        }
    }

    #[test]
    fn alignment_of_translation_array() {
        let arr = [TranslationTableHeader::zeroed(); 4];
        let base_addr = &arr[0] as *const _ as usize;
        for i in 1..4 {
            let addr = &arr[i] as *const _ as usize;
            assert_eq!((addr - base_addr) % align_of::<TranslationTableHeader>(), 0);
        }
    }

    // ---------- new test: check that magic is at offset 0 ----------
    #[test]
    fn magic_at_offset_zero() {
        let hdr = FileHeader {
            magic: *MAGIC,
            version: 0,
            variant_id: 0,
            infoset_count: 0,
            max_actions_k: 0,
            _padding: [0; 7],
        };
        let bytes = bytemuck::bytes_of(&hdr);
        assert_eq!(&bytes[0..8], b"PKRSOTA1");
    }
}
