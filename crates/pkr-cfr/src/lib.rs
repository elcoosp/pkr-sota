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
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Per-dispatch timing breakdown. Returned by `run_iterations_parallel`
/// so callers can log/aggregate without poking at env vars.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunStats {
    pub traverse_s: f64,
    pub merge_s: f64,
    pub flush_s: f64,
    pub chunk_min_s: f64,
    pub chunk_max_s: f64,
    pub chunk_mean_s: f64,
    pub items: usize,
    pub strats: usize,
}

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
    pub fn run_iterations_parallel(&mut self, n: usize) -> RunStats {
        use std::time::Instant;

        // Reserve the whole iteration range with ONE atomic op.
        let start_iter = self.iteration.fetch_add(n as u32, Ordering::Relaxed) + 1;

        let table = Arc::clone(&self.table);
        let abstraction = Arc::clone(&self.abstraction);
        let evaluator = Arc::clone(&self.evaluator);

        // Dynamic chunking: many small chunks, rayon work-steals them
        // across threads. Absorbs per-chunk cost variance and the P-core
        // vs E-core speed gap on M-series.
        const CHUNK_ITERS: usize = 16;
        let n_chunks = (n + CHUNK_ITERS - 1) / CHUNK_ITERS;

        let t_wall = Instant::now();
        let t0 = Instant::now();
        // Each chunk returns (batch, strategy, wall_secs) so imbalance is
        // visible in RunStats.
        let chunk_results: Vec<(Vec<BatchItem>, Vec<StrategyOp>, f64)> =
            (0..n_chunks)
                .into_par_iter()
                .map(|chunk_idx| {
                    let chunk_t0 = Instant::now();
                    let start = chunk_idx * CHUNK_ITERS;
                    let end = ((chunk_idx + 1) * CHUNK_ITERS).min(n);
                    let pairs = end - start;
                    let mut batch: Vec<BatchItem> =
                        Vec::with_capacity(pairs * 20);
                    let mut strategy_batch: Vec<StrategyOp> =
                        Vec::with_capacity(pairs * 20);

                    let mut rng = SmallRng::seed_from_u64(rand::random::<u64>());
                    let initial_deck: [u8; 52] = core::array::from_fn(|i| i as u8);

                    let base_iter = start_iter + start as u32;
                    for local_i in 0..pairs {
                        let global_iter = base_iter + local_i as u32;

                        let mut deck = initial_deck;
                        for i in 0..9usize {
                            let j = i + rng.random_range(0..(52 - i));
                            deck.swap(i, j);
                        }
                        let hero = [deck[0], deck[1]];
                        let villain = [deck[2], deck[3]];
                        let deck_slice = &deck[4..9];

                        let mut state = GameState::new(200.0, 1.0, 2.0);
                        state.set_hole_cards(hero, villain);
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
                    let secs = chunk_t0.elapsed().as_secs_f64();
                    (batch, strategy_batch, secs)
                })
                .collect();
        let t_traverse = t0.elapsed();

        // Chunk imbalance stats.
        let chunk_walls: Vec<f64> = chunk_results.iter().map(|(_, _, s)| *s).collect();
        let chunk_min = chunk_walls.iter().cloned().fold(f64::INFINITY, f64::min);
        let chunk_max = chunk_walls.iter().cloned().fold(0.0f64, f64::max);
        let chunk_mean = if chunk_walls.is_empty() {
            0.0
        } else {
            chunk_walls.iter().sum::<f64>() / chunk_walls.len() as f64
        };

        let t1 = Instant::now();
        let total_items: usize = chunk_results.iter().map(|(b, _, _)| b.len()).sum();
        let total_strats: usize = chunk_results.iter().map(|(_, s, _)| s.len()).sum();
        let mut merged_batch = Vec::with_capacity(total_items);
        let mut merged_strategy = Vec::with_capacity(total_strats);
        for (b, s, _) in chunk_results {
            merged_batch.extend(b);
            merged_strategy.extend(s);
        }
        let t_merge = t1.elapsed();

        let t2 = Instant::now();
        table.apply_strategy_batch(&mut merged_strategy);
        table.flush_cpu_batch(&mut merged_batch);
        let t_flush = t2.elapsed();

        let _ = t_wall; // kept in case a caller wants the total

        RunStats {
            traverse_s: t_traverse.as_secs_f64(),
            merge_s: t_merge.as_secs_f64(),
            flush_s: t_flush.as_secs_f64(),
            chunk_min_s: chunk_min,
            chunk_max_s: chunk_max,
            chunk_mean_s: chunk_mean,
            items: total_items,
            strats: total_strats,
        }
    }

    /// Single-iteration convenience wrapper. Delegates to the batched
    /// implementation with n=1.
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
