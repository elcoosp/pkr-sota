pub mod dcfr;
pub mod table;
pub mod traversal;

use crate::table::CompactRegretTable;
use pkr_contracts::{AbstractionBuilder, Evaluator, GameRules};
use rand::Rng;
use rand::seq::SliceRandom;

pub struct Trainer {
    rules: Box<dyn GameRules>,
    abstraction: Box<dyn AbstractionBuilder>,
    evaluator: Box<dyn Evaluator>,
    table: CompactRegretTable,
    iteration: u32,
}

impl Trainer {
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

    pub fn run_iteration(&mut self, hole: &[u8], rng: &mut impl Rng) {
        self.iteration += 1;

        let mut deck: Vec<u8> = (0..52u8).filter(|c| !hole.contains(c)).collect();
        deck.shuffle(rng);
        let opp_hole = deck[..2].to_vec();
        let board = deck[2..7].to_vec();

        traversal::run_iteration(
            &*self.rules,
            &mut self.table,
            &*self.abstraction,
            &*self.evaluator,
            hole,
            &opp_hole,
            &board,
            rng,
            self.iteration,
            0,
        );
        traversal::run_iteration(
            &*self.rules,
            &mut self.table,
            &*self.abstraction,
            &*self.evaluator,
            hole,
            &opp_hole,
            &board,
            rng,
            self.iteration,
            1,
        );
    }

    pub fn get_table(&self) -> &CompactRegretTable {
        &self.table
    }
}
