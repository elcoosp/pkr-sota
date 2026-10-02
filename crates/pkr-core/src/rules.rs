use pkr_contracts::GameRules;

/// The ruleset for No-Limit Hold'em.
#[derive(Debug, Clone, Copy)]
pub struct NlheRuleset;

impl GameRules for NlheRuleset {
    fn max_actions_per_node(&self) -> u8 {
        6 // fold, check/call, 3 bet sizings, jam — the abstract bucket count
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
            6,
            "max_actions_per_node must be 6"
        );
        assert_eq!(rules.deck_size(), 52, "deck_size must be 52");
        assert_eq!(rules.hand_size(), 2, "hand_size must be 2");
    }
}
