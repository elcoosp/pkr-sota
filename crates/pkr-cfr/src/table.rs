const MIDPOINT: i32 = 128;

/// Compact regret table storing regrets as `u8` quantized values.
/// The midpoint 128 represents zero regret. Values are clamped to 0..255.
pub struct CompactRegretTable {
    num_actions: usize,
    /// Flat array of length `capacity * num_actions`.
    /// Row-major: infoset i, action j -> `i * num_actions + j`.
    regrets: Vec<u8>,
}

impl CompactRegretTable {
    /// Creates a new table for `capacity` information sets, each with `num_actions` actions.
    /// All regrets are initialised to the midpoint (zero regret).
    pub fn new(capacity: usize, num_actions: usize) -> Self {
        Self {
            num_actions,
            regrets: vec![MIDPOINT as u8; capacity * num_actions],
        }
    }

    /// Adds `delta` regret to the specified action of an information set.
    /// Delta can be positive or negative; the stored value is clamped to 0..255.
    pub fn add_regret(&mut self, infoset_idx: usize, action_idx: usize, delta: i32) {
        let offset = infoset_idx * self.num_actions + action_idx;
        let current = self.regrets[offset] as i32;
        let new_val = (current + delta).clamp(0, 255);
        self.regrets[offset] = new_val as u8;
    }

    /// Returns a normalised strategy (vector of probabilities) for the given information set.
    /// Uses the follow-the-leader / regret‑matching rule:
    /// probabilities are proportional to max(0, regret).
    /// If all positive regrets are zero, a uniform distribution is returned.
    pub fn get_strategy(&self, infoset_idx: usize) -> Vec<f32> {
        let start = infoset_idx * self.num_actions;
        let slice = &self.regrets[start..start + self.num_actions];
        let raw_regrets: Vec<i32> = slice.iter().map(|&r| r as i32 - MIDPOINT).collect();
        let positive: Vec<f32> = raw_regrets
            .iter()
            .map(|&r| if r > 0 { r as f32 } else { 0.0 })
            .collect();
        let sum: f32 = positive.iter().sum();
        if sum > 0.0 {
            positive.iter().map(|&p| p / sum).collect()
        } else {
            vec![1.0 / self.num_actions as f32; self.num_actions]
        }
    }

    /// Returns the raw stored `u8` regret value (for testing).
    pub fn get_regret(&self, infoset_idx: usize, action_idx: usize) -> u8 {
        self.regrets[infoset_idx * self.num_actions + action_idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_table_initialises_to_midpoint() {
        let table = CompactRegretTable::new(2, 4);
        for i in 0..2 {
            for a in 0..4 {
                assert_eq!(table.get_regret(i, a), 128, "infoset {i}, action {a}");
            }
        }
    }

    #[test]
    fn add_positive_regret_increases_value() {
        let mut table = CompactRegretTable::new(1, 3);
        table.add_regret(0, 1, 10);
        assert_eq!(table.get_regret(0, 1), 138);
    }

    #[test]
    fn add_negative_regret_decreases_value() {
        let mut table = CompactRegretTable::new(1, 3);
        table.add_regret(0, 0, -10);
        assert_eq!(table.get_regret(0, 0), 118);
    }

    #[test]
    fn quantisation_clamps_to_zero() {
        let mut table = CompactRegretTable::new(1, 2);
        table.add_regret(0, 0, -200); // should clamp to 0
        assert_eq!(table.get_regret(0, 0), 0);
    }

    #[test]
    fn quantisation_clamps_to_255() {
        let mut table = CompactRegretTable::new(1, 2);
        table.add_regret(0, 0, 200); // should clamp to 255
        assert_eq!(table.get_regret(0, 0), 255);
    }

    #[test]
    fn strategy_all_midpoint_gives_uniform() {
        let table = CompactRegretTable::new(1, 5);
        let strat = table.get_strategy(0);
        assert_eq!(strat.len(), 5);
        let expected = 1.0 / 5.0;
        for p in strat {
            assert!((p - expected).abs() < 1e-6, "expected {expected}, got {p}");
        }
    }

    #[test]
    fn strategy_positive_regret_increases_probability() {
        let mut table = CompactRegretTable::new(1, 2);
        // Action 0 gets positive regret, action 1 stays at zero.
        table.add_regret(0, 0, 10);
        let strat = table.get_strategy(0);
        assert!(
            strat[0] > strat[1],
            "Action 0 should have higher probability, got {:?}",
            strat
        );
    }

    #[test]
    fn strategy_sums_to_one() {
        let mut table = CompactRegretTable::new(1, 3);
        table.add_regret(0, 0, 5);
        table.add_regret(0, 1, -3); // negative, ignored in positive sum
        table.add_regret(0, 2, 0);
        let strat = table.get_strategy(0);
        let sum: f32 = strat.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "sum was {sum}");
    }
}
