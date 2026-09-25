pub trait GameRules: Send + Sync {
    fn max_actions_per_node(&self) -> u8;
    fn deck_size(&self) -> usize;
    fn hand_size(&self) -> usize;
}

pub struct InfoSet {
    pub hash: u64,
    pub valid_actions: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
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

    /// Optional soft variant: for hands near a bucket boundary, return
    /// the primary hash plus an adjacent-bucket secondary hash and the
    /// weight of the primary. Consumers that want smooth inference can
    /// blend the two strategies. Default returns a hard assignment.
    ///
    /// Used by `pkr-exploit` to evaluate the effect of smoothing the
    /// trained blueprint without retraining. Gated behind
    /// `PKR_SOFT_KMEANS=1` at the `KMeansAbstraction` level so the
    /// training path is bit-identical when the env var is unset.
    fn get_infoset_hash_soft(
        &self,
        hole: &[u8],
        board: &[u8],
        history: &[u8],
        street: u8,
    ) -> SoftHash {
        SoftHash::hard(self.get_infoset_hash(hole, board, history, street))
    }
}

/// Soft assignment: primary is the hard hash; when `weight_primary < 1.0`,
/// `secondary` holds an adjacent-bucket hash that should be blended in
/// with weight `1.0 - weight_primary`.
#[derive(Debug, Clone, Copy)]
pub struct SoftHash {
    pub primary: u64,
    pub secondary: u64,
    pub weight_primary: f32,
}

impl SoftHash {
    #[inline]
    pub fn hard(primary: u64) -> Self {
        Self { primary, secondary: primary, weight_primary: 1.0 }
    }

    #[inline]
    pub fn is_soft(&self) -> bool {
        self.weight_primary < 1.0 && self.primary != self.secondary
    }
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
