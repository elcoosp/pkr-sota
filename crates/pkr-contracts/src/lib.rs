pub trait GameRules: Send + Sync {
    fn max_actions_per_node(&self) -> u8;
    fn deck_size(&self) -> usize;
    fn hand_size(&self) -> usize;
}

pub struct InfoSet {
    pub hash: u64,
    pub valid_actions: Vec<u8>,
}

/// A blueprint's advice at one infoset: a CDF over abstract action
/// buckets.
///
/// # Invariants
///
/// - `len <= 16` (`cdf_probabilities.len()`).
/// - `cdf_probabilities[..len]` is non-decreasing.
/// - `cdf_probabilities[len - 1] == 255` (closes at certainty).
/// - `len >= 1`.
///
/// Consumers may assume the above and index `[..len]` directly. A
/// violating advice is a producer bug; see `validate()`.
#[derive(Debug, Clone, Copy)]
pub struct SotaAdvice {
    pub cdf_probabilities: [u8; 16], // max 16 actions
    pub len: u8,
}

impl SotaAdvice {
    /// Verify the invariants above. Returns Err(reason) on a violation.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.len == 0 {
            return Err("len must be >= 1");
        }
        if self.len > 16 {
            return Err("len exceeds cdf_probabilities capacity (16)");
        }
        let n = self.len as usize;
        for i in 1..n {
            if self.cdf_probabilities[i] < self.cdf_probabilities[i - 1] {
                return Err("CDF must be non-decreasing");
            }
        }
        if self.cdf_probabilities[n - 1] != 255 {
            return Err("final CDF entry must be 255");
        }
        Ok(())
    }
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
        Self {
            primary,
            secondary: primary,
            weight_primary: 1.0,
        }
    }

    #[inline]
    pub fn is_soft(&self) -> bool {
        self.weight_primary < 1.0 && self.primary != self.secondary
    }
}

/// FNV-1a 64-bit — fully specified, endianness-explicit, stable across
/// compilers/versions/platforms. Used for all infoset hashing so that blueprints
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

#[cfg(test)]
mod audit_2026_11_tests {
    //! Known-answer vectors for the FNV-1a-64 core. The infoset hash is
    //! the compatibility contract between trainer and runtime (blueprint
    //! keys are these hashes), so the primitive gets standard test
    //! vectors pinning it against every other FNV implementation.

    use super::*;

    #[test]
    fn fnv1a_known_answer_vectors() {
        let mut h = FNV_OFFSET;
        // Empty input leaves the offset basis untouched.
        assert_eq!(h, 0xcbf29ce484222325);
        fnv1a(&mut h, b"");
        assert_eq!(h, 0xcbf29ce484222325, "empty input is the identity");

        // Standard FNV-1a 64 test vectors.
        let mut h = FNV_OFFSET;
        fnv1a(&mut h, b"a");
        assert_eq!(h, 0xaf63dc4c8601ec8c);

        let mut h = FNV_OFFSET;
        fnv1a(&mut h, b"foobar");
        assert_eq!(h, 0x85944171f73967e8);

        let mut h = FNV_OFFSET;
        fnv1a(&mut h, b"hello");
        assert_eq!(h, 0xa430d84680aabd0b);
    }

    /// The hash is sequential: hashing a then b equals hashing the
    /// concatenation, and byte order matters.
    #[test]
    fn fnv1a_streaming_and_order_sensitivity() {
        let mut whole = FNV_OFFSET;
        fnv1a(&mut whole, b"street1sigcluster");

        let mut split = FNV_OFFSET;
        fnv1a(&mut split, b"street1");
        fnv1a(&mut split, b"sigcluster");
        assert_eq!(whole, split, "streaming must equal one-shot");

        let mut swapped = FNV_OFFSET;
        fnv1a(&mut swapped, b"1street");
        fnv1a(&mut swapped, b"sigcluster");
        assert_ne!(whole, swapped, "byte order must matter");
    }

    /// The constants are the FNV-1a 64 spec values — pin them so an
    /// accidental "tweak" breaks loudly here instead of silently
    /// invalidating every blueprint.
    #[test]
    fn fnv_constants_are_spec_values() {
        assert_eq!(FNV_OFFSET, 0xcbf29ce484222325);
        assert_eq!(FNV_PRIME, 0x100000001b3);
        assert_eq!(HASH_ALGO_FNV1A64_INFOSET, 2);
    }
}
