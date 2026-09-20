#![allow(clippy::manual_hash_one)]
pub mod fmph;
pub mod header;
pub mod translate;
pub mod writer;


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
        let fh = FileHeader {
            magic: *b"PKRSOTA1",
            version: 2,
            variant_id: 0,
            infoset_count: 100,
            max_actions_k: 6,
            hash_algo: HASH_ALGO_FNV1A64_INFOSET,
            _padding: [0; 6],
        };
        let bytes = bytemuck::bytes_of(&fh);
        let decoded: &FileHeader = bytemuck::from_bytes(bytes);
        assert_eq!(decoded.infoset_count, 100);
        assert_eq!(decoded.max_actions_k, 6);
    }
}
