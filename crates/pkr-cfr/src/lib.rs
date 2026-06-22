pub mod dcfr;
pub mod table;
pub mod traversal;

use crate::table::CompactRegretTable;
use crate::traversal::traverse;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::GameState;
use rayon::prelude::*;
use std::sync::Arc;

pub struct Trainer {
    abstraction: Arc<dyn AbstractionBuilder>,
    evaluator: Arc<dyn Evaluator>,
    table: CompactRegretTable,
    iteration: u32,
}

impl Trainer {
    pub fn new(
        abstraction: Arc<dyn AbstractionBuilder>,
        evaluator: Arc<dyn Evaluator>,
        num_actions: usize,
    ) -> Self {
        Self {
            abstraction,
            evaluator,
            table: CompactRegretTable::new(num_actions),
            iteration: 0,
        }
    }

    pub fn run_iterations_parallel(
        &mut self,
        state: &GameState,
        chance_cards: &[Vec<u8>; 3],
        num_iterations: u32,
        num_threads: usize,
    ) {
        let abstraction = Arc::clone(&self.abstraction);
        let evaluator = Arc::clone(&self.evaluator);
        let tables: Vec<CompactRegretTable> = (0..num_threads)
            .into_par_iter()
            .map(|_| {
                let mut thread_table = CompactRegretTable::new(self.table.num_actions());
                let mut rng = rand::rng();
                let state_copy = state.clone();
                let chance_copy = chance_cards.clone();
                for _ in 0..num_iterations {
                    traverse(
                        &state_copy,
                        &mut thread_table,
                        &*abstraction,
                        &*evaluator,
                        &mut rng,
                        1,
                        0,
                        1.0,
                        1.0,
                        &chance_copy,
                    );
                    traverse(
                        &state_copy,
                        &mut thread_table,
                        &*abstraction,
                        &*evaluator,
                        &mut rng,
                        1,
                        1,
                        1.0,
                        1.0,
                        &chance_copy,
                    );
                }
                thread_table
            })
            .collect();

        for t in tables {
            self.table.merge(&t);
        }
        self.iteration += num_iterations * num_threads as u32;
    }

    pub fn get_table(&self) -> &CompactRegretTable {
        &self.table
    }
}
