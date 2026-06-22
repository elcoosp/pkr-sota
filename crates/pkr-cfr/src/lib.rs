pub mod dcfr;
pub mod table;
pub mod traversal;

use crate::table::CompactRegretTable;
use crate::traversal::traverse;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::GameState;
use rand::Rng;

pub struct Trainer {
    abstraction: Box<dyn AbstractionBuilder>,
    evaluator: Box<dyn Evaluator>,
    table: CompactRegretTable,
    iteration: u32,
}

impl Trainer {
    pub fn new(
        abstraction: Box<dyn AbstractionBuilder>,
        evaluator: Box<dyn Evaluator>,
        num_actions: usize,
    ) -> Self {
        Self {
            abstraction,
            evaluator,
            table: CompactRegretTable::new(num_actions),
            iteration: 0,
        }
    }

    pub fn run_iteration(&mut self, state: &GameState, rng: &mut impl Rng) {
        self.iteration += 1;
        // Traverse from perspective of both players
        let state_copy = state.clone();
        // Resolve chance nodes (deal flop/turn/river) - already done in state
        traverse(
            &state_copy,
            &mut self.table,
            &*self.abstraction,
            &*self.evaluator,
            rng,
            self.iteration,
            0,
            1.0,
            1.0,
        );
        traverse(
            &state_copy,
            &mut self.table,
            &*self.abstraction,
            &*self.evaluator,
            rng,
            self.iteration,
            1,
            1.0,
            1.0,
        );
    }

    pub fn get_table(&self) -> &CompactRegretTable {
        &self.table
    }
}
