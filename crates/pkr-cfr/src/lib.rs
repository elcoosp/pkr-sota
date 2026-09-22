pub mod dcfr;
pub mod gpu;
pub mod preflop_validate;
pub mod riversolve;
pub mod table;
pub mod traversal;
pub mod valuenet;

use crate::gpu::BatchItem;
use crate::table::CompactRegretTable;
use crate::traversal::traverse;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::GameState;
use rand::seq::SliceRandom;
use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub struct Trainer {
    abstraction: Arc<dyn AbstractionBuilder>,
    evaluator: Arc<dyn Evaluator>,
    table: Arc<CompactRegretTable>,
    iteration: AtomicU32,
}

impl Trainer {
    pub fn new(abstraction: Arc<dyn AbstractionBuilder>, evaluator: Arc<dyn Evaluator>) -> Self {
        Self::with_capacity(abstraction, evaluator, 5_000_000)
    }

    pub fn with_capacity(
        abstraction: Arc<dyn AbstractionBuilder>,
        evaluator: Arc<dyn Evaluator>,
        capacity: usize,
    ) -> Self {
        Self {
            abstraction,
            evaluator,
            table: Arc::new(CompactRegretTable::with_capacity(capacity)),
            iteration: AtomicU32::new(0),
        }
    }

    pub fn run_iteration_parallel(&mut self) {
        let global_iter = self.iteration.fetch_add(1, Ordering::Relaxed) + 1;
        let table = Arc::clone(&self.table);
        let abstraction = Arc::clone(&self.abstraction);
        let evaluator = Arc::clone(&self.evaluator);

        // Each thread collects its own batch
        let thread_batches: Vec<Vec<BatchItem>> = (0..rayon::current_num_threads())
            .into_par_iter()
            .map(|_| {
                let mut batch = Vec::with_capacity(10000);
                let mut rng = rand::rng();
                let mut deck: Vec<u8> = (0..52).collect();
                deck.shuffle(&mut rng);
                let hero = [deck[0], deck[1]];
                let villain = [deck[2], deck[3]];

                let mut state = GameState::new(200.0, 1.0, 2.0);
                state.set_hole_cards(hero, villain);
                let deck_slice = &deck[4..];
                let mut deck_idx = 0usize;
                traverse(
                    &mut state,
                    &table,
                    &*abstraction,
                    &*evaluator,
                    &mut rng,
                    global_iter,
                    0,
                    1.0,
                    1.0,
                    deck_slice,
                    &mut deck_idx,
                    0,
                    &mut batch,
                );

                let mut state2 = GameState::new(200.0, 1.0, 2.0);
                state2.set_hole_cards(hero, villain);
                let mut deck_idx2 = 0usize;
                traverse(
                    &mut state2,
                    &table,
                    &*abstraction,
                    &*evaluator,
                    &mut rng,
                    global_iter,
                    1,
                    1.0,
                    1.0,
                    deck_slice,
                    &mut deck_idx2,
                    0,
                    &mut batch,
                );
                batch
            })
            .collect();

        // Merge all batches into one giant batch
        let mut merged_batch = Vec::with_capacity(100_000);
        for tb in thread_batches {
            merged_batch.extend(tb);
        }

        // CPU flush: no WGPU submit, no sync, no staging buffer. The DCFR
        // math is a handful of flops per item; the sync overhead of the GPU
        // path dominates for HU NLHE with K=6.
        table.flush_cpu_batch(&merged_batch);
    }

    pub fn get_table(&self) -> &CompactRegretTable {
        &self.table
    }

    pub fn iteration(&self) -> u32 {
        self.iteration.load(Ordering::Relaxed)
    }

    pub fn is_near_capacity(&self) -> bool {
        let cap = self.table.capacity();
        cap > 0 && self.table.len() * 100 / cap >= 95
    }

    pub fn save_checkpoint(&self, path: &str) -> std::io::Result<()> {
        self.table.save_checkpoint(path, self.iteration())
    }

    pub fn load_checkpoint(&self, path: &str) -> std::io::Result<()> {
        let iter = self.table.load_checkpoint(path)?;
        self.iteration.store(iter, Ordering::Relaxed);
        Ok(())
    }
}
