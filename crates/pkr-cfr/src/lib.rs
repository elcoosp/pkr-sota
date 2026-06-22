pub mod dcfr;
pub mod table;
pub mod traversal;

use crate::table::CompactRegretTable;
use crate::traversal::traverse;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::GameState;
use rand::seq::SliceRandom;
use rayon::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct Trainer {
    abstraction: Arc<dyn AbstractionBuilder>,
    evaluator: Arc<dyn Evaluator>,
    table: CompactRegretTable,
    iteration: AtomicU32,
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
            iteration: AtomicU32::new(0),
        }
    }

    /// Run one iteration of external-sampling MCCFR using all available threads.
    /// Relies on the global Rayon thread pool (configured in main).
    pub fn run_iteration_parallel(&mut self) {
        let global_iter = self.iteration.fetch_add(1, Ordering::Relaxed) + 1;

        let tables: Vec<CompactRegretTable> = (0..rayon::current_num_threads())
            .into_par_iter()
            .map(|_| {
                let mut thread_table = CompactRegretTable::new(6); // K=6 abstract actions
                let mut rng = rand::rng();
                let mut deck: Vec<u8> = (0..52).collect();
                deck.shuffle(&mut rng);
                let hero = [deck[0], deck[1]];
                let villain = [deck[2], deck[3]];
                let mut state = GameState::new(200.0, 1.0, 2.0);
                state.set_hole_cards(hero, villain);
                let mut remaining = deck[4..].to_vec();
                traverse(
                    &state,
                    &mut thread_table,
                    &*self.abstraction,
                    &*self.evaluator,
                    &mut rng,
                    global_iter,
                    0,
                    1.0,
                    1.0,
                    &mut remaining,
                );
                let mut state2 = GameState::new(200.0, 1.0, 2.0);
                state2.set_hole_cards(hero, villain);
                let mut remaining2 = deck[4..].to_vec();
                traverse(
                    &state2,
                    &mut thread_table,
                    &*self.abstraction,
                    &*self.evaluator,
                    &mut rng,
                    global_iter,
                    1,
                    1.0,
                    1.0,
                    &mut remaining2,
                );
                thread_table
            })
            .collect();

        for t in tables {
            self.table.merge(&t);
        }
    }

    pub fn get_table(&self) -> &CompactRegretTable {
        &self.table
    }
}
