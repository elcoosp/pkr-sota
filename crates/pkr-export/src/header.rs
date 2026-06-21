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
}
