pub mod dcfr;
pub mod table;
pub mod traversal;

use crate::table::CompactRegretTable;
use pkr_contracts::{AbstractionBuilder, Evaluator, GameRules};
use rand::Rng;

/// High-level trainer for Discounted Counterfactual Regret Minimization (DCFR).
///
/// This struct owns the regret table, the game rules, abstraction builder, and evaluator,
/// and provides a simple interface to run DCFR iterations.
///
/// # Example
///
/// ```ignore
/// let mut trainer = Trainer::new(rules, abstraction, evaluator, 1024);
/// for _ in 0..NUM_ITERATIONS {
///     let hole = sample_random_hand();
///     trainer.run_iteration(&hole, &mut rng);
/// }
/// let strategy = trainer.get_table().get_strategy(infoset_idx);
/// ```
pub struct Trainer {
    rules: Box<dyn GameRules>,
    abstraction: Box<dyn AbstractionBuilder>,
    evaluator: Box<dyn Evaluator>,
    table: CompactRegretTable,
    iteration: u32,
}

impl Trainer {
    /// Creates a new `Trainer` with the given rules, abstraction, evaluator, and table capacity.
    ///
    /// The capacity determines how many information sets can be stored in the regret table.
    /// It should be at least the number of distinct information sets expected during training.
    pub fn new(
        rules: Box<dyn GameRules>,
        abstraction: Box<dyn AbstractionBuilder>,
        evaluator: Box<dyn Evaluator>,
        capacity: usize,
    ) -> Self {
        let num_actions = rules.max_actions_per_node() as usize;
        Self {
            rules,
            abstraction,
            evaluator,
            table: CompactRegretTable::new(capacity, num_actions),
            iteration: 0,
        }
    }

    /// Runs one DCFR iteration for the given hand and both players (0 and 1).
    ///
    /// This updates the internal regret table using the discounting rules from `dcfr`.
    /// The iteration counter is incremented before the traversal.
    pub fn run_iteration(&mut self, hole: &[u8], rng: &mut impl Rng) {
        self.iteration += 1;
        // Player 0 traversal
        traversal::run_iteration(
            &*self.rules,
            &mut self.table,
            &*self.abstraction,
            &*self.evaluator,
            hole,
            rng,
            self.iteration,
            0,
        );
        // Player 1 traversal
        traversal::run_iteration(
            &*self.rules,
            &mut self.table,
            &*self.abstraction,
            &*self.evaluator,
            hole,
            rng,
            self.iteration,
            1,
        );
    }

    /// Returns a reference to the internal regret table.
    pub fn get_table(&self) -> &CompactRegretTable {
        &self.table
    }
}

#[cfg(test)]
mod trainer_tests {
    use super::*;
    use pkr_contracts::AbstractionBuilder;
    use pkr_contracts::Evaluator;
    use pkr_contracts::GameRules;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    struct TestRules;
    impl GameRules for TestRules {
        fn max_actions_per_node(&self) -> u8 {
            4
        }
        fn deck_size(&self) -> usize {
            52
        }
        fn hand_size(&self) -> usize {
            2
        }
    }

    struct TestAbstraction;
    impl AbstractionBuilder for TestAbstraction {
        fn get_infoset_hash(&self, _hole: &[u8], _board: &[u8], _history: &[u8]) -> u64 {
            0
        }
    }

    struct TestEvaluator;
    impl Evaluator for TestEvaluator {
        fn evaluate_hand(&self, _hole: &[u8], board: &[u8]) -> u16 {
            if board.len() < 2 {
                0
            } else if board[0] == board[1] {
                1
            } else {
                0
            }
        }
    }

    #[test]
    fn trainer_run_iteration_updates_table() {
        let rules = Box::new(TestRules);
        let abstraction = Box::new(TestAbstraction);
        let evaluator = Box::new(TestEvaluator);
        let mut trainer = Trainer::new(rules, abstraction, evaluator, 2);
        let hole = vec![0u8, 0];
        let mut rng = StdRng::seed_from_u64(42);
        let before = trainer.get_table().get_regret(0, 0);
        assert_eq!(before, 128, "Initial regret should be 128");
        trainer.run_iteration(&hole, &mut rng);
        let after = trainer.get_table().get_regret(0, 0);
        assert!(
            after != 128,
            "Regret should change after iteration, but was still 128"
        );
    }
}
