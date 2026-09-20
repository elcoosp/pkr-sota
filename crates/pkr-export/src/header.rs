use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct FileHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub variant_id: u32,
    pub infoset_count: u64,
    pub max_actions_k: u8,
    pub hash_algo: u8,
    pub _padding: [u8; 6],
}

/// Hash algorithm identifiers stored in FileHeader.hash_algo.
pub const HASH_ALGO_FNV1A64_INFOSET: u8 = pkr_contracts::HASH_ALGO_FNV1A64_INFOSET;
pub const FORMAT_VERSION_V2: u32 = 2; // version that introduced hash_algo field

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct FmphHeader {
    pub num_keys: u64,
    pub seed1: u64,
    pub seed2: u64,
    pub max_level_size: u64,
    pub level_count: u32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct TranslationTableHeader {
    pub num_entries: u64,
    pub action_size: u32,
    pub _padding: [u8; 4],
}

// Compile-time size guards
const _: () = assert!(std::mem::size_of::<FileHeader>() == 32);
const _: () = assert!(std::mem::size_of::<FmphHeader>() == 40);
const _: () = assert!(std::mem::size_of::<TranslationTableHeader>() == 16);
