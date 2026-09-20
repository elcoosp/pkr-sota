pub trait GameRules: Send + Sync {
    fn max_actions_per_node(&self) -> u8;
    fn deck_size(&self) -> usize;
    fn hand_size(&self) -> usize;
}

pub struct InfoSet {
    pub hash: u64,
    pub valid_actions: Vec<u8>,
}

pub struct SotaAdvice {
    pub cdf_probabilities: [u8; 16], // max 16 actions
    pub len: u8,
}

pub trait BlueprintProvider: Send + Sync {
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice>;
}

pub trait Evaluator: Send + Sync {
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u32;
}

pub trait AbstractionBuilder: Send + Sync {
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8], street: u8) -> u64;
}

/// FNV-1a 64-bit — fully specified, endianness-explicit, stable across
/// compilers/versions/platforms. Used for all infoset hashing so that bluepints
/// trained on one machine load correctly on another. DO NOT replace with
/// DefaultHasher/RandomState.
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64-bit (endianness-explicit). Stable across platforms/compilers.
#[inline]
pub fn fnv1a(state: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *state ^= b as u64;
        *state = state.wrapping_mul(FNV_PRIME);
    }
}

/// Hash algorithm identifier written into blueprint FileHeader.
/// 1 = legacy DefaultHasher (rejected at load); 2 = FNV-1a 64-bit.
pub const HASH_ALGO_FNV1A64_INFOSET: u8 = 2;
pub const HASH_ALGO_LEGACY_DEFAULT: u8 = 1;
