use bytemuck::{Pod, Zeroable};

/// File header written at offset 0 of every blueprint.
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

/// v2: introduced hash_algo field to reject legacy DefaultHasher blueprints.
pub const FORMAT_VERSION_V2: u32 = 2;

/// v3: introduced a 48-byte AnchorsSection immediately after FileHeader.
/// Layout: [FileHeader:32][AnchorsSection:48][key_count:u32][cdf_size:u32][keys][cdf]
pub const FORMAT_VERSION_V3: u32 = 3;

/// Minimal perfect hash header. Currently unused by the writer, kept for
/// the future O(1) lookup path.
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

/// Translation table header. Currently unused by the writer, kept for the
/// future precomputed-translation path.
#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct TranslationTableHeader {
    pub num_entries: u64,
    pub action_size: u32,
    pub _padding: [u8; 4],
}

/// Per-street bet-size anchors written after FileHeader in v3 blueprints.
/// Values are pot fractions (e.g. 0.5 for half-pot). The runtime uses
/// these to bracket an off-tree opponent bet between the two anchors the
/// trainer was trained with (pseudo-harmonic, Ganzfried & Sandholm 2013).
#[repr(C)]
#[derive(Debug, Copy, Clone, Pod, Zeroable)]
pub struct AnchorsSection {
    /// [preflop, flop, turn, river] × [small, medium, large] in pot fractions.
    /// Preflop row is all zeros (no partial bet sizes preflop in this abstraction).
    pub anchors: [[f32; 3]; 4],
}

/// The concrete anchors used by the trainer. Mirror of
/// `crates/pkr-core/src/state.rs`'s bet-size loop. When those change,
/// regenerate abstraction tables and re-export.
pub const ANCHORS: [[f32; 3]; 4] = [
    [0.0, 0.0, 0.0],
    [0.4, 0.8, 1.6],
    [0.4, 0.8, 1.6],
    [0.4, 0.8, 1.6],
];

// Compile-time size guards
const _: () = assert!(std::mem::size_of::<FileHeader>() == 32);
const _: () = assert!(std::mem::size_of::<FmphHeader>() == 40);
const _: () = assert!(std::mem::size_of::<TranslationTableHeader>() == 16);
const _: () = assert!(std::mem::size_of::<AnchorsSection>() == 48);
