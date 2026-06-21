/// Defines the rules of a poker variant.
/// All implementors must be thread‑safe (`Send + Sync`).
pub trait GameRules: Send + Sync {
    /// Maximum number of legal actions at any decision point.
    fn max_actions_per_node(&self) -> u8;
    /// Number of cards in the deck (usually 52).
    fn deck_size(&self) -> usize;
    /// Number of hole cards dealt to each player.
    fn hand_size(&self) -> usize;
}

/// An information set identified by a hash and a list of valid actions.
pub struct InfoSet {
    pub hash: u64,
    pub valid_actions: Vec<u8>,
}

/// Pre‑computed strategy advice in the form of a CDF (0‑255 bytes).
pub struct SotaAdvice {
    /// Each byte represents a cumulative probability bucket.
    pub cdf_probabilities: Vec<u8>,
}

/// Lookup interface for pre‑flop or flop blueprints.
/// Implementors must be thread‑safe.
pub trait BlueprintProvider: Send + Sync {
    /// Returns the advice for the given information set, if available.
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice>;
}

/// Evaluates a poker hand and returns its rank (lower = stronger).
/// Implementors must be thread‑safe.
pub trait Evaluator: Send + Sync {
    /// `hole` is the player's hole cards, `board` is the community cards.
    fn evaluate_hand(&self, hole: &[u8], board: &[u8]) -> u16;
}

/// Abstraction builder that maps a game state to an information set hash.
/// Implementors must be thread‑safe.
pub trait AbstractionBuilder: Send + Sync {
    /// Produces a hash that uniquely identifies the information set.
    fn get_infoset_hash(&self, hole: &[u8], board: &[u8], history: &[u8]) -> u64;
}

#[cfg(test)]
mod tests {
    use crate::{AbstractionBuilder, BlueprintProvider, Evaluator, GameRules, InfoSet, SotaAdvice};

    struct MockRules;
    struct MockEval;
    struct MockAbstraction;
    struct MockBlueprint;

    impl GameRules for MockRules {
        fn max_actions_per_node(&self) -> u8 {
            5
        }
        fn deck_size(&self) -> usize {
            52
        }
        fn hand_size(&self) -> usize {
            2
        }
    }

    impl Evaluator for MockEval {
        fn evaluate_hand(&self, _hole: &[u8], _board: &[u8]) -> u16 {
            0
        }
    }

    impl AbstractionBuilder for MockAbstraction {
        fn get_infoset_hash(&self, _hole: &[u8], _board: &[u8], _history: &[u8]) -> u64 {
            0
        }
    }

    impl BlueprintProvider for MockBlueprint {
        fn lookup(&self, _infoset_hash: u64) -> Option<SotaAdvice> {
            Some(SotaAdvice {
                cdf_probabilities: vec![128],
            })
        }
    }

    #[test]
    fn test_mock_implements_traits() {
        let rules = MockRules;
        assert_eq!(rules.max_actions_per_node(), 5);
        assert_eq!(rules.deck_size(), 52);
        assert_eq!(rules.hand_size(), 2);

        let eval = MockEval;
        assert_eq!(eval.evaluate_hand(&[], &[]), 0);

        let abs = MockAbstraction;
        assert_eq!(abs.get_infoset_hash(&[], &[], &[]), 0);

        let bp = MockBlueprint;
        let advice = bp.lookup(0).unwrap();
        assert_eq!(advice.cdf_probabilities, vec![128]);
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockRules>();
        assert_send_sync::<MockEval>();
        assert_send_sync::<MockAbstraction>();
        assert_send_sync::<MockBlueprint>();
        assert_send_sync::<InfoSet>();
        assert_send_sync::<SotaAdvice>();
    }
}
