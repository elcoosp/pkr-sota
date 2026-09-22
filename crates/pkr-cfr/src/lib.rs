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

    /// Runs `n` logical CFR iterations per rayon dispatch instead of one.
    ///
    /// Each rayon task runs its share of iterations locally (no sync
    /// between them), accumulating into buffers allocated once for the
    /// whole call. Merge + flush happens ONCE per dispatch instead of once
    /// per iteration. This is the fix for the 1->N thread collapse: the old
    /// code paid a full serial merge+flush every iteration, and that serial
    /// cost grew *with* thread count since more threads = more items
    /// produced per iteration. Batching amortizes it by `n`.
    pub fn run_iterations_parallel(&mut self, n: usize) {
        use std::sync::OnceLock;
        use std::time::Instant;
        static PROFILE: OnceLock<bool> = OnceLock::new();
        let profile = *PROFILE.get_or_init(|| std::env::var("PKR_PHASE_PROFILE").is_ok());

        let num_threads = rayon::current_num_threads().max(1);
        // Reserve the whole iteration-number range with ONE atomic op
        // instead of one fetch_add per traversal pair.
        let start_iter = self.iteration.fetch_add(n as u32, Ordering::Relaxed) + 1;

        let table = Arc::clone(&self.table);
        let abstraction = Arc::clone(&self.abstraction);
        let evaluator = Arc::clone(&self.evaluator);

        // Split n pairs as evenly as possible across threads.
        let base = n / num_threads;
        let rem = n % num_threads;

        let t_wall = Instant::now();
        let t0 = Instant::now();
        let thread_results: Vec<(Vec<BatchItem>, Vec<StrategyOp>)> =
            (0..num_threads)
                .into_par_iter()
                .map(|t| {
                    let pairs_for_this_thread: usize = base + if t < rem { 1 } else { 0 };
                    // Size the buffer for the whole local batch, not one pair,
                    // so we allocate ONCE per dispatch instead of once per pair.
                    let mut batch: Vec<BatchItem> =
                        Vec::with_capacity(pairs_for_this_thread * 20);
                    let mut strategy_batch: Vec<StrategyOp> =
                        Vec::with_capacity(pairs_for_this_thread * 20);
                    let mut rng = rand::rng();

                    // Offset into the reserved iteration-number range.
                    let thread_start = start_iter + (t * base + t.min(rem)) as u32;

                    for local_i in 0..pairs_for_this_thread {
                        let global_iter = thread_start + local_i as u32;

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
                    }
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
        table.apply_strategy_batch(&merged_strategy);
        table.flush_cpu_batch(&merged_batch);
        let t_flush = t2.elapsed();

        if profile {
            let wall_ms = t_wall.elapsed().as_secs_f64() * 1000.0;
            eprintln!(
                "[phase] batch_end_iter={} n={} wall={:.2}ms traverse={:.2}ms merge={:.2}ms flush={:.2}ms items={} strats={}",
                start_iter + n as u32 - 1,
                n,
                wall_ms,
                t_traverse.as_secs_f64() * 1000.0,
                t_merge.as_secs_f64() * 1000.0,
                t_flush.as_secs_f64() * 1000.0,
                total_items,
                total_strats,
            );
        }
    }

    /// Single-iteration convenience wrapper. Delegates to the batched
    /// implementation with n=1. Kept so any external caller still works.
    pub fn run_iteration_parallel(&mut self) {
        self.run_iterations_parallel(1);
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
