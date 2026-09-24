#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here

pub mod dcfr;
pub mod gpu;
pub mod metrics;
pub mod preflop_validate;
pub mod riversolve;
pub mod table;
pub mod traversal;
pub mod valuenet;

use crate::gpu::BatchItem;
use crate::metrics::LocalMetrics;
use crate::table::{CompactRegretTable, StrategyOp};
use crate::traversal::traverse;
use pkr_contracts::{AbstractionBuilder, Evaluator};
use pkr_core::state::GameState;
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub struct Trainer {
    abstraction: Arc<dyn AbstractionBuilder>,
    evaluator: Arc<dyn Evaluator>,
    table: Arc<CompactRegretTable>,
    iteration: AtomicU32,
    run_seed: u64,
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
            run_seed: 0x5EED_1F70,
        }
    }

    pub fn set_run_seed(&mut self, seed: u64) {
        self.run_seed = seed;
    }

    /// Runs `n` logical CFR iterations per rayon dispatch. Each rayon task
    /// runs its share of iterations locally, accumulating into buffers
    /// allocated once for the whole call. Merge + flush happens ONCE per
    /// dispatch, amortizing serial work by `n`.
    #[allow(clippy::type_complexity)]
    pub fn run_iterations_parallel(&mut self, n: usize) {
        use crate::metrics::global;
        use std::sync::OnceLock;
        use std::time::Instant;
        static PROFILE: OnceLock<bool> = OnceLock::new();
        let profile = *PROFILE.get_or_init(|| std::env::var("PKR_PHASE_PROFILE").is_ok());

        let start_iter = self.iteration.fetch_add(n as u32, Ordering::Relaxed) + 1;

        let table = Arc::clone(&self.table);
        let abstraction = Arc::clone(&self.abstraction);
        let evaluator = Arc::clone(&self.evaluator);
        let run_seed = self.run_seed;

        const CHUNK_ITERS: usize = 16;
        let n_chunks = n.div_ceil(CHUNK_ITERS);

        let t_wall = Instant::now();
        let t0 = Instant::now();
        let thread_results: Vec<(Vec<BatchItem>, Vec<StrategyOp>, LocalMetrics)> = (0..n_chunks)
            .into_par_iter()
            .map(|chunk_idx| {
                let start = chunk_idx * CHUNK_ITERS;
                let end = ((chunk_idx + 1) * CHUNK_ITERS).min(n);
                let pairs = end - start;
                let mut batch: Vec<BatchItem> = Vec::with_capacity(pairs * 20);
                let mut strategy_batch: Vec<StrategyOp> = Vec::with_capacity(pairs * 20);
                let mut metrics = LocalMetrics::default();

                let base_iter = start_iter + start as u32;
                let mut rng = SmallRng::seed_from_u64(
                    run_seed
                        ^ (base_iter as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        ^ ((chunk_idx as u64) << 32),
                );
                let initial_deck: [u8; 52] = core::array::from_fn(|i| i as u8);

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
                        deck_slice,
                        &mut deck_idx,
                        0,
                        &mut batch,
                        &mut strategy_batch,
                        &mut metrics,
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
                        deck_slice,
                        &mut deck_idx2,
                        0,
                        &mut batch,
                        &mut strategy_batch,
                        &mut metrics,
                    );
                }

                metrics.regret_pushed = batch.len() as u64;
                metrics.strategy_pushed = strategy_batch.len() as u64;
                (batch, strategy_batch, metrics)
            })
            .collect();
        let t_traverse = t0.elapsed();

        let t1 = Instant::now();
        let total_items: usize = thread_results.iter().map(|(b, _, _)| b.len()).sum();
        let total_strats: usize = thread_results.iter().map(|(_, s, _)| s.len()).sum();
        let mut merged_batch = Vec::with_capacity(total_items);
        let mut merged_strategy = Vec::with_capacity(total_strats);
        let mut batch_metrics = LocalMetrics::default();
        for entry in thread_results.iter() {
            merged_batch.extend_from_slice(&entry.0);
            merged_strategy.extend_from_slice(&entry.1);
            batch_metrics.merge_from(&entry.2);
        }
        let t_merge = t1.elapsed();

        let t2 = Instant::now();
        let strategy_applied = table.apply_strategy_batch(&mut merged_strategy);
        let (regret_in, regret_out) = table.flush_cpu_batch(&mut merged_batch);
        let t_flush = t2.elapsed();

        global().record_batch(
            &batch_metrics,
            n as u64,
            t_wall.elapsed().as_nanos() as u64,
            t_traverse.as_nanos() as u64,
            t_merge.as_nanos() as u64,
            t_flush.as_nanos() as u64,
            regret_in,
            regret_out,
            strategy_applied,
        );

        if profile {
            let wall_ms = t_wall.elapsed().as_secs_f64() * 1000.0;
            let hit_rate = if batch_metrics.cache_hits + batch_metrics.cache_misses == 0 {
                0.0
            } else {
                batch_metrics.cache_hits as f64
                    / (batch_metrics.cache_hits + batch_metrics.cache_misses) as f64
            };
            eprintln!(
                "[phase] batch_end_iter={} n={} chunks={} wall={:.2}ms traverse={:.2}ms merge={:.2}ms flush={:.2}ms items={} strats={} nodes={} depth_max={} cache_hit={:.3}",
                start_iter + n as u32 - 1,
                n,
                n_chunks,
                wall_ms,
                t_traverse.as_secs_f64() * 1000.0,
                t_merge.as_secs_f64() * 1000.0,
                t_flush.as_secs_f64() * 1000.0,
                total_items,
                total_strats,
                batch_metrics.nodes,
                batch_metrics.max_depth,
                hit_rate,
            );
        }
    }

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
        cap > 0 && self.table.allocated() * 100 / cap >= 95
    }

    pub fn save_checkpoint(
        &self,
        path: &str,
        fingerprint: &pkr_core::abstraction::AbstractionFingerprint,
    ) -> std::io::Result<()> {
        self.table
            .save_checkpoint(path, self.iteration(), fingerprint)
    }

    pub fn load_checkpoint(
        &self,
        path: &str,
        current: &pkr_core::abstraction::AbstractionFingerprint,
    ) -> std::io::Result<()> {
        let iter = self.table.load_checkpoint(path, current)?;
        self.iteration.store(iter, Ordering::Relaxed);
        Ok(())
    }
}
