use pkr_contracts::GameRules;

/// NLHE ruleset stub – intentionally wrong values to make tests fail.
pub struct NlheRuleset;

impl GameRules for NlheRuleset {
    fn max_actions_per_node(&self) -> u8 {
        0 // should be 4, will be corrected in green phase
    }
    fn deck_size(&self) -> usize {
        0 // should be 52
    }
    fn hand_size(&self) -> usize {
        0 // should be 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nlhe_ruleset_implements_game_rules_correctly() {
        let rules = NlheRuleset;
        assert_eq!(rules.max_actions_per_node(), 4, "max_actions_per_node must be 4");
        assert_eq!(rules.deck_size(), 52, "deck_size must be 52");
        assert_eq!(rules.hand_size(), 2, "hand_size must be 2");
    }
}
