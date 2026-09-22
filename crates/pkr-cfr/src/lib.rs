pub mod dcfr;
pub mod gpu;
pub mod preflop_validate;
pub mod riversolve;
pub mod table;
pub mod traversal;
pub mod valuenet;

use crate::gpu::BatchItem;
use crate::table::{CompactRegretTable, StrategyOp};
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
        use std::sync::OnceLock;
        use std::time::Instant;
        static PROFILE: OnceLock<bool> = OnceLock::new();
        let profile = *PROFILE.get_or_init(|| std::env::var("PKR_PHASE_PROFILE").is_ok());

        let global_iter = self.iteration.fetch_add(1, Ordering::Relaxed) + 1;
        let table = Arc::clone(&self.table);
        let abstraction = Arc::clone(&self.abstraction);
        let evaluator = Arc::clone(&self.evaluator);

        let t0 = Instant::now();
        let thread_results: Vec<(Vec<BatchItem>, Vec<StrategyOp>)> =
            (0..rayon::current_num_threads())
                .into_par_iter()
                .map(|_| {
                    let mut batch = Vec::with_capacity(10000);
                    let mut strategy_batch = Vec::with_capacity(10000);
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
                        &mut strategy_batch,
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
                        &mut strategy_batch,
                    );
                    (batch, strategy_batch)
                })
                .collect();
        let t_traverse = t0.elapsed();

        let t1 = Instant::now();
        let total_items: usize = thread_results.iter().map(|(b, _)| b.len()).sum();
        let total_strats: usize = thread_results.iter().map(|(_, s)| s.len()).sum();
        let mut merged_batch = Vec::with_capacity(total_items);
        let mut merged_strategy = Vec::with_capacity(total_strats);
        for (b, s) in thread_results {
            merged_batch.extend(b);
            merged_strategy.extend(s);
        }
        let t_merge = t1.elapsed();

        let t2 = Instant::now();
        for op in &merged_strategy {
            table.add_strategy_sum_at(op.index as usize, op.action as usize, op.prob);
        }
        table.flush_cpu_batch(&merged_batch);
        let t_flush = t2.elapsed();

        if profile && global_iter % 5000 == 0 {
            eprintln!(
                "[phase] iter={} traverse={:.2}ms merge={:.2}ms flush={:.2}ms items={} strats={}",
                global_iter,
                t_traverse.as_secs_f64() * 1000.0,
                t_merge.as_secs_f64() * 1000.0,
                t_flush.as_secs_f64() * 1000.0,
                total_items,
                total_strats,
            );
        }
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
