use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct FileHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub variant_id: u32,
    pub infoset_count: u64,
    pub max_actions_k: u8,
    pub _padding: [u8; 7],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct FmphHeader {
    pub num_keys: u64,
    pub seed1: u64,
    pub seed2: u64,
    pub max_level_size: u64,
    pub level_count: u32,
    pub _padding: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TranslationTableHeader {
    pub num_entries: u64,
    pub action_size: u32,
    pub _padding: [u8; 4],
}
