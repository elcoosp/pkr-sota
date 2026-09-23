//! Training-time instrumentation.
//!
//! Two tiers:
//!
//! 1. `LocalMetrics`: plain u64 counters, no atomics, passed by &mut
//!    through the traversal. Each rayon task accumulates into its own
//!    instance. Merge into `GlobalMetrics` at batch boundaries. The
//!    per-node write is one field increment, no locking.
//!
//! 2. `GlobalMetrics`: atomics, updated once per batch. Snapshot and
//!    delta give rolling-window values for CSV output.
//!
//! Nothing here affects the CFR algorithm. All counters are
//! observational only.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

/// Per-thread, per-batch counters. Passed by &mut through traverse.
/// No atomics, no locks. Cost per update is a single field increment.
#[derive(Clone, Default)]
pub struct LocalMetrics {
    pub nodes: u64,
    pub depth_sum: u64,
    pub max_depth: u32,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub infosets_created: u64,
    pub strategy_pushed: u64,
    pub regret_pushed: u64,
    /// C5d: incremented in `traverse` when `depth > MAX_DEPTH`. Any
    /// nonzero value means a hand ran deeper than the traverser can
    /// handle and the returned 0.0 corrupted the regret math.
    pub depth_overflows: u64,
    /// C5d: incremented when `*deck_idx > deck.len()` mid-runout.
    /// Same class of silent-0.0 bug as depth_overflows.
    pub deck_overflows: u64,
    pub depth_hist: [u32; 32],
}

impl LocalMetrics {
    #[inline(always)]
    pub fn record_node(&mut self, depth: u32) {
        self.nodes += 1;
        self.depth_sum += depth as u64;
        if depth > self.max_depth {
            self.max_depth = depth;
        }
        let slot = (depth as usize).min(31);
        self.depth_hist[slot] = self.depth_hist[slot].saturating_add(1);
    }

    pub fn merge_from(&mut self, other: &LocalMetrics) {
        self.nodes += other.nodes;
        self.depth_sum += other.depth_sum;
        self.max_depth = self.max_depth.max(other.max_depth);
        self.cache_hits += other.cache_hits;
        self.cache_misses += other.cache_misses;
        self.infosets_created += other.infosets_created;
        self.strategy_pushed += other.strategy_pushed;
        self.regret_pushed += other.regret_pushed;
        self.depth_overflows += other.depth_overflows;
        self.deck_overflows += other.deck_overflows;
        for i in 0..32 {
            self.depth_hist[i] = self.depth_hist[i].saturating_add(other.depth_hist[i]);
        }
    }
}

/// Cumulative process-wide counters. Updated once per batch by the
/// coordinator thread; never touched by rayon workers.
pub struct GlobalMetrics {
    pub nodes: AtomicU64,
    pub depth_sum: AtomicU64,
    pub max_depth: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub infosets_created: AtomicU64,
    pub strategy_pushed: AtomicU64,
    pub regret_pushed: AtomicU64,
    pub depth_overflows: AtomicU64,
    pub deck_overflows: AtomicU64,
    pub strategy_applied: AtomicU64,
    pub regret_input: AtomicU64,
    pub regret_unique: AtomicU64,
    pub batches: AtomicU64,
    pub iterations: AtomicU64,
    pub wall_ns: AtomicU64,
    pub traverse_ns: AtomicU64,
    pub merge_ns: AtomicU64,
    pub flush_ns: AtomicU64,
    pub depth_hist: [AtomicU64; 32],
}

impl GlobalMetrics {
    fn new() -> Self {
        Self {
            nodes: AtomicU64::new(0),
            depth_sum: AtomicU64::new(0),
            max_depth: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            infosets_created: AtomicU64::new(0),
            strategy_pushed: AtomicU64::new(0),
            regret_pushed: AtomicU64::new(0),
            depth_overflows: AtomicU64::new(0),
            deck_overflows: AtomicU64::new(0),
            strategy_applied: AtomicU64::new(0),
            regret_input: AtomicU64::new(0),
            regret_unique: AtomicU64::new(0),
            batches: AtomicU64::new(0),
            iterations: AtomicU64::new(0),
            wall_ns: AtomicU64::new(0),
            traverse_ns: AtomicU64::new(0),
            merge_ns: AtomicU64::new(0),
            flush_ns: AtomicU64::new(0),
            depth_hist: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }

    /// Fold a batch's LocalMetrics and phase timings into the global
    /// totals. Called once per `run_iterations_parallel` invocation.
    #[allow(clippy::too_many_arguments)]
    pub fn record_batch(
        &self,
        m: &LocalMetrics,
        iterations: u64,
        wall_ns: u64,
        traverse_ns: u64,
        merge_ns: u64,
        flush_ns: u64,
        regret_in: u64,
        regret_out: u64,
        strategy_in: u64,
    ) {
        self.nodes.fetch_add(m.nodes, Ordering::Relaxed);
        self.depth_sum.fetch_add(m.depth_sum, Ordering::Relaxed);
        self.max_depth
            .fetch_max(m.max_depth as u64, Ordering::Relaxed);
        self.cache_hits.fetch_add(m.cache_hits, Ordering::Relaxed);
        self.cache_misses
            .fetch_add(m.cache_misses, Ordering::Relaxed);
        self.infosets_created
            .fetch_add(m.infosets_created, Ordering::Relaxed);
        self.strategy_pushed
            .fetch_add(m.strategy_pushed, Ordering::Relaxed);
        self.regret_pushed
            .fetch_add(m.regret_pushed, Ordering::Relaxed);
        self.depth_overflows
            .fetch_add(m.depth_overflows, Ordering::Relaxed);
        self.deck_overflows
            .fetch_add(m.deck_overflows, Ordering::Relaxed);
        self.strategy_applied
            .fetch_add(strategy_in, Ordering::Relaxed);
        self.regret_input.fetch_add(regret_in, Ordering::Relaxed);
        self.regret_unique.fetch_add(regret_out, Ordering::Relaxed);
        self.batches.fetch_add(1, Ordering::Relaxed);
        self.iterations.fetch_add(iterations, Ordering::Relaxed);
        self.wall_ns.fetch_add(wall_ns, Ordering::Relaxed);
        self.traverse_ns.fetch_add(traverse_ns, Ordering::Relaxed);
        self.merge_ns.fetch_add(merge_ns, Ordering::Relaxed);
        self.flush_ns.fetch_add(flush_ns, Ordering::Relaxed);
        for i in 0..32 {
            self.depth_hist[i].fetch_add(m.depth_hist[i] as u64, Ordering::Relaxed);
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            nodes: self.nodes.load(Ordering::Relaxed),
            depth_sum: self.depth_sum.load(Ordering::Relaxed),
            max_depth: self.max_depth.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            cache_misses: self.cache_misses.load(Ordering::Relaxed),
            infosets_created: self.infosets_created.load(Ordering::Relaxed),
            strategy_pushed: self.strategy_pushed.load(Ordering::Relaxed),
            regret_pushed: self.regret_pushed.load(Ordering::Relaxed),
            depth_overflows: self.depth_overflows.load(Ordering::Relaxed),
            deck_overflows: self.deck_overflows.load(Ordering::Relaxed),
            strategy_applied: self.strategy_applied.load(Ordering::Relaxed),
            regret_input: self.regret_input.load(Ordering::Relaxed),
            regret_unique: self.regret_unique.load(Ordering::Relaxed),
            batches: self.batches.load(Ordering::Relaxed),
            iterations: self.iterations.load(Ordering::Relaxed),
            wall_ns: self.wall_ns.load(Ordering::Relaxed),
            traverse_ns: self.traverse_ns.load(Ordering::Relaxed),
            merge_ns: self.merge_ns.load(Ordering::Relaxed),
            flush_ns: self.flush_ns.load(Ordering::Relaxed),
            depth_hist: std::array::from_fn(|i| self.depth_hist[i].load(Ordering::Relaxed)),
        }
    }
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub nodes: u64,
    pub depth_sum: u64,
    pub max_depth: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub infosets_created: u64,
    pub strategy_pushed: u64,
    pub regret_pushed: u64,
    pub depth_overflows: u64,
    pub deck_overflows: u64,
    pub strategy_applied: u64,
    pub regret_input: u64,
    pub regret_unique: u64,
    pub batches: u64,
    pub iterations: u64,
    pub wall_ns: u64,
    pub traverse_ns: u64,
    pub merge_ns: u64,
    pub flush_ns: u64,
    pub depth_hist: [u64; 32],
}

impl Snapshot {
    /// Saturating delta, safe even if `prev` was captured after `self`
    /// (impossible here, but the guards cost nothing).
    pub fn delta(&self, prev: &Snapshot) -> Snapshot {
        Snapshot {
            nodes: self.nodes.saturating_sub(prev.nodes),
            depth_sum: self.depth_sum.saturating_sub(prev.depth_sum),
            max_depth: self.max_depth,
            cache_hits: self.cache_hits.saturating_sub(prev.cache_hits),
            cache_misses: self.cache_misses.saturating_sub(prev.cache_misses),
            infosets_created: self.infosets_created.saturating_sub(prev.infosets_created),
            strategy_pushed: self.strategy_pushed.saturating_sub(prev.strategy_pushed),
            regret_pushed: self.regret_pushed.saturating_sub(prev.regret_pushed),
            depth_overflows: self.depth_overflows.saturating_sub(prev.depth_overflows),
            deck_overflows: self.deck_overflows.saturating_sub(prev.deck_overflows),
            strategy_applied: self.strategy_applied.saturating_sub(prev.strategy_applied),
            regret_input: self.regret_input.saturating_sub(prev.regret_input),
            regret_unique: self.regret_unique.saturating_sub(prev.regret_unique),
            batches: self.batches.saturating_sub(prev.batches),
            iterations: self.iterations.saturating_sub(prev.iterations),
            wall_ns: self.wall_ns.saturating_sub(prev.wall_ns),
            traverse_ns: self.traverse_ns.saturating_sub(prev.traverse_ns),
            merge_ns: self.merge_ns.saturating_sub(prev.merge_ns),
            flush_ns: self.flush_ns.saturating_sub(prev.flush_ns),
            depth_hist: std::array::from_fn(|i| {
                self.depth_hist[i].saturating_sub(prev.depth_hist[i])
            }),
        }
    }

    pub fn avg_depth(&self) -> f64 {
        if self.nodes == 0 {
            0.0
        } else {
            self.depth_sum as f64 / self.nodes as f64
        }
    }

    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }

    pub fn regret_dedup_ratio(&self) -> f64 {
        if self.regret_input == 0 {
            0.0
        } else {
            self.regret_unique as f64 / self.regret_input as f64
        }
    }
}

static GLOBAL: OnceLock<GlobalMetrics> = OnceLock::new();

pub fn global() -> &'static GlobalMetrics {
    GLOBAL.get_or_init(GlobalMetrics::new)
}
