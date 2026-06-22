#![allow(clippy::manual_hash_one)]
pub mod fmph;
pub mod header;
pub mod translate;
pub mod writer;
#[cfg(test)]
mod fmph_tests {
    use crate::fmph::{build_fmph, eval_fmph};

    #[test]
    fn test_build_fmph_small_set() {
        let keys: Vec<u64> = (0..100).map(|i| i as u64 * 1000).collect();
        let fmph = build_fmph(&keys);
        for &k in &keys {
            let idx = eval_fmph(&fmph, k);
            assert!(idx < keys.len(), "idx {} out of bounds for key {}", idx, k);
        }
        let mut seen = vec![false; keys.len()];
        for &k in &keys {
            let idx = eval_fmph(&fmph, k);
            assert!(!seen[idx], "duplicate idx {} for key {}", idx, k);
            seen[idx] = true;
        }
    }

    #[test]
    fn test_fmph_single_key() {
        let fmph = build_fmph(&[42]);
        assert_eq!(eval_fmph(&fmph, 42), 0);
    }

    #[test]
    fn test_fmph_duplicate_keys() {
        let keys = vec![1, 2, 2, 3, 1, 1];
        let fmph = build_fmph(&keys);
        assert_eq!(fmph.keys_len, 3);
    }

    #[test]
    fn test_fmph_header_roundtrip() {
        let keys: Vec<u64> = (0..50).collect();
        let fmph = build_fmph(&keys);
        let header = fmph.to_header();
        assert_eq!(header.num_keys, 50);
        assert_eq!(header.seed1, fmph.seed1);
        assert_eq!(header.seed2, fmph.seed2);
        assert_eq!(header.max_level_size, fmph.bucket_count as u64);
        assert_eq!(header.level_count, 1);
    }
}

#[cfg(test)]
mod header_tests {
    use crate::header::*;
    use bytemuck;

    #[test]
    fn test_file_header_size() { assert_eq!(std::mem::size_of::<FileHeader>(), 32); }
    #[test]
    fn test_fmph_header_size() { assert_eq!(std::mem::size_of::<FmphHeader>(), 40); }
    #[test]
    fn test_translation_header_size() { assert_eq!(std::mem::size_of::<TranslationTableHeader>(), 16); }

    #[test]
    fn test_file_header_pod() {
        let fh = FileHeader { magic: *b"PKRSOTA1", version: 1, variant_id: 0, infoset_count: 100, max_actions_k: 6, _padding: [0; 7] };
        let bytes = bytemuck::bytes_of(&fh);
        let decoded: &FileHeader = bytemuck::from_bytes(bytes);
        assert_eq!(decoded.infoset_count, 100);
        assert_eq!(decoded.max_actions_k, 6);
    }
}
