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

    /// Returns the number of actions per information set.
    pub fn num_actions(&self) -> usize {
        self.num_actions
    }

    /// Returns the total number of information sets this table can hold.
    pub fn capacity(&self) -> usize {
        self.regrets.len() / self.num_actions
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

    #[test]
    fn strategy_with_all_negative_regrets_is_uniform() {
        let mut table = CompactRegretTable::new(1, 3);
        table.add_regret(0, 0, -10);
        table.add_regret(0, 1, -20);
        table.add_regret(0, 2, -5);
        let strat = table.get_strategy(0);
        let expected = 1.0 / 3.0;
        for p in strat {
            assert!((p - expected).abs() < 1e-6, "expected {expected}, got {p}");
        }
    }

    #[test]
    fn strategy_proportional_to_positive_regret() {
        let mut table = CompactRegretTable::new(1, 3);
        // Only action 0 and 2 have positive regret.
        table.add_regret(0, 0, 20);
        table.add_regret(0, 1, -100);
        table.add_regret(0, 2, 40);
        let strat = table.get_strategy(0);
        // Action 0: regret 20, action2: 40 -> probabilities: 20/60 = 0.333..., 40/60 = 0.666...
        assert!((strat[0] - (20.0 / 60.0)).abs() < 1e-6, "action0");
        assert!((strat[1] - 0.0).abs() < 1e-6, "action1 should be 0");
        assert!((strat[2] - (40.0 / 60.0)).abs() < 1e-6, "action2");
        let sum: f32 = strat.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6, "sum should be 1");
    }

    #[test]
    fn add_regret_accumulates_over_multiple_calls() {
        let mut table = CompactRegretTable::new(1, 2);
        table.add_regret(0, 0, 10);
        table.add_regret(0, 0, 5);
        assert_eq!(table.get_regret(0, 0), 143); // 128 + 15
    }

    #[test]
    fn adding_zero_delta_does_not_change_regret() {
        let mut table = CompactRegretTable::new(1, 2);
        let initial = table.get_regret(0, 0);
        table.add_regret(0, 0, 0);
        assert_eq!(table.get_regret(0, 0), initial);
    }

    #[test]
    fn multiple_infosets_are_independent() {
        let mut table = CompactRegretTable::new(3, 3);
        table.add_regret(0, 1, 10);
        table.add_regret(1, 2, -5);
        table.add_regret(2, 0, 20);
        assert_eq!(table.get_regret(0, 1), 138);
        assert_eq!(table.get_regret(0, 2), 128); // unchanged
        assert_eq!(table.get_regret(1, 2), 123);
        assert_eq!(table.get_regret(1, 0), 128); // unchanged
        assert_eq!(table.get_regret(2, 0), 148);
    }

    #[test]
    fn storage_size_matches_capacity_times_actions() {
        let cap = 5;
        let acts = 7;
        let table = CompactRegretTable::new(cap, acts);
        assert_eq!(table.capacity(), cap);
        assert_eq!(table.num_actions(), acts);
        // internal vec length is exactly cap * acts
        // We can verify via get_regret on last index doesn't panic
        let last = (cap - 1) * acts + (acts - 1);
        let _ = table.get_regret(cap - 1, acts - 1);
    }

    #[test]
    fn get_strategy_returns_vector_of_correct_length() {
        let table = CompactRegretTable::new(1, 4);
        let strat = table.get_strategy(0);
        assert_eq!(strat.len(), 4);
    }

    #[test]
    fn one_byte_per_action_per_infoset() {
        // Verify that each stored value is a u8 (size_of == 1)
        assert_eq!(std::mem::size_of::<u8>(), 1);
        // Verify internal vector length matches capacity * num_actions
        let table = CompactRegretTable::new(10, 4);
        // capacity() gives 10, num_actions() gives 4, so total bytes = 40
        // We can't directly access regrets.len() without a method, but we have get_regret.
        // We'll just assert that capacity() * num_actions() == 40.
        assert_eq!(table.capacity() * table.num_actions(), 40);
    }
}
