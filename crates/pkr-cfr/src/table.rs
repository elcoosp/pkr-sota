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
        let new_val = current.saturating_add(delta).clamp(0, 255);
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
        if self.num_actions == 0 {
            0
        } else {
            self.regrets.len() / self.num_actions
        }
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
        table.add_regret(0, 0, -200);
        assert_eq!(table.get_regret(0, 0), 0);
    }

    #[test]
    fn quantisation_clamps_to_255() {
        let mut table = CompactRegretTable::new(1, 2);
        table.add_regret(0, 0, 200);
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
        table.add_regret(0, 0, 10);
        let strat = table.get_strategy(0);
        assert!(
            strat[0] > strat[1],
            "Action 0 should have higher probability"
        );
    }

    #[test]
    fn strategy_sums_to_one() {
        let mut table = CompactRegretTable::new(1, 3);
        table.add_regret(0, 0, 5);
        table.add_regret(0, 1, -3);
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
            assert!((p - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn strategy_proportional_to_positive_regret() {
        let mut table = CompactRegretTable::new(1, 3);
        table.add_regret(0, 0, 20);
        table.add_regret(0, 1, -100);
        table.add_regret(0, 2, 40);
        let strat = table.get_strategy(0);
        assert!((strat[0] - (20.0 / 60.0)).abs() < 1e-6);
        assert!((strat[1] - 0.0).abs() < 1e-6);
        assert!((strat[2] - (40.0 / 60.0)).abs() < 1e-6);
        let sum: f32 = strat.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn add_regret_accumulates_over_multiple_calls() {
        let mut table = CompactRegretTable::new(1, 2);
        table.add_regret(0, 0, 10);
        table.add_regret(0, 0, 5);
        assert_eq!(table.get_regret(0, 0), 143);
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
        assert_eq!(table.get_regret(0, 2), 128);
        assert_eq!(table.get_regret(1, 2), 123);
        assert_eq!(table.get_regret(1, 0), 128);
        assert_eq!(table.get_regret(2, 0), 148);
    }

    #[test]
    fn storage_size_matches_capacity_times_actions() {
        let table = CompactRegretTable::new(5, 7);
        assert_eq!(table.capacity(), 5);
        assert_eq!(table.num_actions(), 7);
        let _ = table.get_regret(4, 6);
    }

    #[test]
    fn get_strategy_returns_vector_of_correct_length() {
        let table = CompactRegretTable::new(1, 4);
        assert_eq!(table.get_strategy(0).len(), 4);
    }

    #[test]
    fn one_byte_per_action_per_infoset() {
        assert_eq!(std::mem::size_of::<u8>(), 1);
        let table = CompactRegretTable::new(10, 4);
        assert_eq!(table.capacity() * table.num_actions(), 40);
    }

    #[test]
    fn extreme_positive_regret_gives_probability_one() {
        let mut table = CompactRegretTable::new(1, 3);
        table.add_regret(0, 1, 127);
        let strat = table.get_strategy(0);
        assert!((strat[1] - 1.0).abs() < 1e-6);
        assert!((strat[0] - 0.0).abs() < 1e-6);
        assert!((strat[2] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn clamped_value_can_be_reduced() {
        let mut table = CompactRegretTable::new(1, 1);
        table.add_regret(0, 0, 200);
        assert_eq!(table.get_regret(0, 0), 255);
        table.add_regret(0, 0, -10);
        assert_eq!(table.get_regret(0, 0), 245);
        table.add_regret(0, 0, -300);
        assert_eq!(table.get_regret(0, 0), 0);
        table.add_regret(0, 0, 15);
        assert_eq!(table.get_regret(0, 0), 15);
    }

    #[test]
    fn large_capacity_and_actions() {
        let cap = 1000;
        let acts = 10;
        let mut table = CompactRegretTable::new(cap, acts);
        table.add_regret(0, 0, 50);
        table.add_regret(cap - 1, acts - 1, -30);
        assert_eq!(table.get_regret(0, 0), 178);
        assert_eq!(table.get_regret(cap - 1, acts - 1), 98);
        assert_eq!(table.get_regret(500, 5), 128);
    }

    #[test]
    fn strategy_is_consistent_across_calls() {
        let mut table = CompactRegretTable::new(1, 2);
        table.add_regret(0, 0, 10);
        let s1 = table.get_strategy(0);
        let s2 = table.get_strategy(0);
        assert_eq!(s1.len(), s2.len());
        for (a, b) in s1.iter().zip(s2.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn num_actions_and_capacity_methods_work() {
        let table = CompactRegretTable::new(7, 3);
        assert_eq!(table.num_actions(), 3);
        assert_eq!(table.capacity(), 7);
    }

    #[test]
    fn regret_never_exceeds_u8_range() {
        let mut table = CompactRegretTable::new(1, 1);
        for _ in 0..10 {
            table.add_regret(0, 0, 100);
        }
        assert_eq!(table.get_regret(0, 0), 255);
        for _ in 0..10 {
            table.add_regret(0, 0, -100);
        }
        assert_eq!(table.get_regret(0, 0), 0);
    }

    #[test]
    fn strategy_with_only_one_positive_regret_works() {
        let mut table = CompactRegretTable::new(1, 4);
        table.add_regret(0, 2, 50);
        let strat = table.get_strategy(0);
        assert!((strat[2] - 1.0).abs() < 1e-6);
        assert!((strat[0] - 0.0).abs() < 1e-6);
        assert!((strat[1] - 0.0).abs() < 1e-6);
        assert!((strat[3] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn add_regret_with_delta_exceeding_i32_range_should_clamp() {
        let mut table = CompactRegretTable::new(1, 1);
        table.add_regret(0, 0, i32::MAX);
        assert_eq!(table.get_regret(0, 0), 255);
        let mut table = CompactRegretTable::new(1, 1);
        table.add_regret(0, 0, i32::MIN);
        assert_eq!(table.get_regret(0, 0), 0);
    }

    #[test]
    fn strategy_probabilities_are_non_negative() {
        let mut table = CompactRegretTable::new(1, 3);
        table.add_regret(0, 0, 10);
        table.add_regret(0, 1, -5);
        table.add_regret(0, 2, 20);
        let strat = table.get_strategy(0);
        for &p in &strat {
            assert!(p >= 0.0, "probability {p} should be non-negative");
        }
    }

    #[test]
    fn zero_capacity_table_has_correct_dimensions() {
        let table = CompactRegretTable::new(0, 5);
        assert_eq!(table.capacity(), 0);
        assert_eq!(table.num_actions(), 5);
    }

    #[test]
    fn zero_capacity_zero_actions_table_creation_does_not_panic() {
        let table = CompactRegretTable::new(0, 0);
        assert_eq!(table.capacity(), 0);
        assert_eq!(table.num_actions(), 0);
    }

    #[test]
    fn get_strategy_with_different_infosets_is_independent() {
        let mut table = CompactRegretTable::new(3, 2);
        table.add_regret(0, 0, 30);
        table.add_regret(1, 1, -10);
        table.add_regret(2, 0, 10);
        let s0 = table.get_strategy(0);
        let s1 = table.get_strategy(1);
        let s2 = table.get_strategy(2);
        assert!(s0[0] > s0[1]);
        assert!((s1[0] - 0.5).abs() < 1e-6 && (s1[1] - 0.5).abs() < 1e-6);
        assert!(s2[0] > s2[1]);
    }

    #[test]
    fn strategy_all_zero_after_reset_to_midpoint() {
        let mut table = CompactRegretTable::new(1, 4);
        table.add_regret(0, 1, 50);
        table.add_regret(0, 1, -50);
        let strat = table.get_strategy(0);
        let expected = 1.0 / 4.0;
        for p in &strat {
            assert!((p - expected).abs() < 1e-6);
        }
    }
}
