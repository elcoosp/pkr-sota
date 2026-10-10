use pkr_contracts::GameRules;

/// The ruleset for No-Limit Hold'em.
#[derive(Debug, Clone, Copy)]
pub struct NlheRuleset;

impl GameRules for NlheRuleset {
    fn max_actions_per_node(&self) -> u8 {
        // M-2 (audit 2026-10-10): fold, check/call, 4 bet sizings
        // (BET_SIZINGS has length 4), jam = 7. The docstring used to say 6
        // (K=6 stale). Actual bound: NUM_ACTION_BUCKETS.
        crate::abstraction::NUM_ACTION_BUCKETS as u8
    }

    fn deck_size(&self) -> usize {
        52
    }

    fn hand_size(&self) -> usize {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nlhe_ruleset_implements_game_rules_correctly() {
        let rules = NlheRuleset;
        assert_eq!(
            rules.max_actions_per_node(),
            crate::abstraction::NUM_ACTION_BUCKETS as u8,
            "max_actions_per_node must equal NUM_ACTION_BUCKETS"
        );
        assert_eq!(rules.deck_size(), 52, "deck_size must be 52");
        assert_eq!(rules.hand_size(), 2, "hand_size must be 2");
    }
}
