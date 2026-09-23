// T1.1: flush_cpu_batch now uses the integer path
// (crate::dcfr::update_regret_i64). The f32 wrapper is kept for A/B
// comparison and tests.

use crate::gpu::{BatchItem, GpuState};
use crate::metrics::LocalMetrics;
use foldhash::fast::RandomState as FoldHasher;
use papaya::HashMap as PapayaMap;
use rayon::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

const K: usize = 6;
/// Regret + momentum interleaved: [r0 m0 r1 m1 r2 m2 r3 m3 r4 m4 r5 m5]
/// Written only by the coordinator (flush_cpu_batch), so no false sharing.
const RM_FIELDS: usize = 2;
const RM_STRIDE: usize = K * RM_FIELDS;
const RM_REGRET: usize = 0;
const RM_MOMENTUM: usize = 1;

/// Strategy sums live in a separate array indexed [s0..s5].
/// Stored as f64 bits in AtomicU64 — see `add_sum` for why fixed-point was
/// wrong here: reach_prob decays multiplicatively through the tree, and at
/// depth ~5 it drops below 1e-3. A fixed-point i64 with SCALE=1000 truncates
/// those contributions to zero, silently leaving 68% of deep infosets with
/// uniform strategies in the exported blueprint. f64 has no such floor.
const SUM_STRIDE: usize = K;

pub(crate) const SCALE: f32 = 1000.0;

#[derive(Clone, Copy, Debug)]
pub struct StrategyOp {
    pub index: u32,
    pub action: u8,
    pub prob: f32,
}

#[derive(Debug, Clone, Default)]
pub struct TableSnapshot {
    pub infosets: usize,
    pub capacity: usize,
    pub max_abs_regret: f32,
    pub mean_abs_regret: f32,
    pub nonfinite_count: usize,
    pub strategy_sum_mass: f64,
}

#[derive(Debug, Clone)]
pub struct StrategyAnalysis {
    pub total: usize,
    /// Strategy sum total == 0.0 (never visited or truncated to zero).
    pub empty: usize,
    /// One action has p >= 0.99.
    pub pure: usize,
    /// At least two actions have p >= 0.10.
    pub mixed: usize,
    /// Shannon entropy in bits, mean over visited infosets.
    pub mean_entropy: f64,
    /// Entropy histogram in 8 buckets of 0.25 bits, up to 2.0+.
    pub entropy_histogram: [usize; 8],
    /// For each action, how many infosets have it as the argmax.
    pub dominant_counts: [usize; K],
    /// Count of (idx, action) cells whose strategy_sum is > 0.
    pub nonzero_strategy_sum_cells: usize,
    /// Of the visited infosets, how many fell all the way through to the
    /// uniform last resort (flat regrets AND zero strategy sum). These
    /// are the infosets where we have genuinely no signal, and they are
    /// the ones that should worry you if the count is high.
    pub uniform_fallback: usize,
}

#[derive(Debug, Clone)]
pub struct InfoSetDump {
    pub hash: u64,
    pub strategy: [f32; K],
    pub regrets: [f32; K],
}

const IDX_CACHE_INIT: usize = 1 << 18;
const IDX_CACHE_MAX: usize = 1 << 20;

thread_local! {
    static IDX_CACHE: RefCell<HashMap<u64, usize, FoldHasher>> =
        RefCell::new(HashMap::with_capacity_and_hasher(
            IDX_CACHE_INIT,
            FoldHasher::default(),
        ));
}

#[inline]
fn cache_lookup(hash: u64) -> Option<usize> {
    IDX_CACHE.with(|c| c.borrow().get(&hash).copied())
}

#[inline]
fn cache_insert(hash: u64, idx: usize) {
    IDX_CACHE.with(|c| {
        let mut m = c.borrow_mut();
        if m.len() >= IDX_CACHE_MAX {
            m.clear();
        }
        m.insert(hash, idx);
    });
}

/// Retained for future use if the integer path ever needs to warn about
/// non-finite intermediates (currently it cannot produce them because
/// all arithmetic stays in i64/i128).
#[allow(dead_code)]
fn warn_nonfinite_regret_once(iteration: u32) {
    use std::sync::OnceLock;
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!(
            "WARNING: regret became non-finite at iteration {}. \
             Training is corrupt from this point.",
            iteration
        );
    });
}

pub struct CompactRegretTable {
    hash_to_idx: PapayaMap<u64, usize, FoldHasher>,
    /// Interleaved regret+momentum, i32 fixed-point at scale 1000.
    data: Vec<AtomicI32>,
    /// f64 strategy sums stored as u64 bits. Independent array to avoid
    /// false sharing with the interleaved regret/momentum data.
    strategy_sum: Vec<AtomicU64>,
    next_idx: AtomicUsize,
    capacity: usize,
    gpu: OnceLock<GpuState>,
}

impl Default for CompactRegretTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CompactRegretTable {
    pub fn new() -> Self {
        Self::with_capacity(5_000_000)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let mut data: Vec<AtomicI32> = Vec::with_capacity(capacity * RM_STRIDE);
        data.resize_with(capacity * RM_STRIDE, || AtomicI32::new(0));
        let mut strategy_sum: Vec<AtomicU64> = Vec::with_capacity(capacity * SUM_STRIDE);
        strategy_sum.resize_with(capacity * SUM_STRIDE, || AtomicU64::new(0));
        let map = PapayaMap::with_hasher(FoldHasher::default());
        map.pin().reserve(4_000_000.min(capacity));
        Self {
            hash_to_idx: map,
            data,
            strategy_sum,
            next_idx: AtomicUsize::new(0),
            capacity,
            gpu: OnceLock::new(),
        }
    }

    #[inline(always)]
    fn off_rm(idx: usize, action: usize, field: usize) -> usize {
        idx * RM_STRIDE + action * RM_FIELDS + field
    }

    #[inline(always)]
    fn off_sum(idx: usize, action: usize) -> usize {
        idx * SUM_STRIDE + action
    }

    #[inline(always)]
    fn load_rm(&self, idx: usize, action: usize, field: usize) -> i32 {
        self.data[Self::off_rm(idx, action, field)].load(Ordering::Relaxed)
    }

    #[inline(always)]
    fn store_rm(&self, idx: usize, action: usize, field: usize, v: i32) {
        self.data[Self::off_rm(idx, action, field)].store(v, Ordering::Relaxed);
    }

    #[inline(always)]
    fn load_sum(&self, idx: usize, action: usize) -> f64 {
        f64::from_bits(self.strategy_sum[Self::off_sum(idx, action)].load(Ordering::Relaxed))
    }

    /// CAS-loop add. In `apply_strategy_batch` each (idx, action) appears
    /// in exactly one parallel group, so the CAS succeeds first try. The
    /// loop exists for `add_strategy_sum_at`, which may be called
    /// concurrently from multiple threads on the same cell.
    #[inline(always)]
    fn add_sum(&self, idx: usize, action: usize, delta: f64) {
        let cell = &self.strategy_sum[Self::off_sum(idx, action)];
        let mut cur_bits = cell.load(Ordering::Relaxed);
        loop {
            let cur = f64::from_bits(cur_bits);
            let new_bits = (cur + delta).to_bits();
            match cell.compare_exchange_weak(
                cur_bits,
                new_bits,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => cur_bits = actual,
            }
        }
    }

    #[inline]
    fn alloc_idx(&self) -> usize {
        loop {
            let cur = self.next_idx.load(Ordering::Relaxed);
            if cur >= self.capacity {
                panic!(
                    "CompactRegretTable capacity {} exceeded. Increase --capacity \
                     or reduce abstraction resolution.",
                    self.capacity
                );
            }
            match self.next_idx.compare_exchange_weak(
                cur,
                cur + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return cur,
                Err(_) => continue,
            }
        }
    }

    pub(crate) fn get_or_create_idx(&self, hash: u64) -> usize {
        let guard = self.hash_to_idx.pin();
        if let Some(idx) = guard.get(&hash) {
            return *idx;
        }
        let fresh = self.alloc_idx();
        match guard.try_insert(hash, fresh) {
            Ok(_) => fresh,
            Err(_) => guard.get(&hash).copied().unwrap_or(fresh),
        }
    }

    #[inline]
    pub(crate) fn get_or_create_idx_measured(
        &self,
        hash: u64,
        metrics: &mut LocalMetrics,
    ) -> usize {
        let guard = self.hash_to_idx.pin();
        if let Some(idx) = guard.get(&hash) {
            return *idx;
        }
        let fresh = self.alloc_idx();
        match guard.try_insert(hash, fresh) {
            Ok(_) => {
                metrics.infosets_created += 1;
                fresh
            }
            Err(_) => guard.get(&hash).copied().unwrap_or(fresh),
        }
    }

    pub fn get_strategy_and_idx(
        &self,
        infoset_hash: u64,
        out: &mut [f32; K],
        metrics: &mut LocalMetrics,
    ) -> usize {
        let idx = match cache_lookup(infoset_hash) {
            Some(i) => {
                metrics.cache_hits += 1;
                i
            }
            None => {
                metrics.cache_misses += 1;
                let i = self.get_or_create_idx_measured(infoset_hash, metrics);
                cache_insert(infoset_hash, i);
                i
            }
        };
        let mut sum = 0.0f32;
        for i in 0..K {
            let raw = self.load_rm(idx, i, RM_REGRET);
            let val = ((raw as f32) / SCALE).max(0.0);
            out[i] = val;
            sum += val;
        }
        if sum > 0.0 {
            let inv = 1.0 / sum;
            for i in 0..K {
                out[i] *= inv;
            }
        } else {
            out.fill(1.0 / K as f32);
        }
        idx
    }

    pub fn get_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        let idx_opt = match cache_lookup(infoset_hash) {
            Some(i) => Some(i),
            None => {
                let guard = self.hash_to_idx.pin();
                let found = guard.get(&infoset_hash).copied();
                drop(guard);
                if let Some(i) = found {
                    cache_insert(infoset_hash, i);
                }
                found
            }
        };
        if let Some(idx) = idx_opt {
            let mut sum = 0.0f32;
            for i in 0..K {
                let raw = self.load_rm(idx, i, RM_REGRET);
                let val = ((raw as f32) / SCALE).max(0.0);
                out[i] = val;
                sum += val;
            }
            if sum > 0.0 {
                let inv = 1.0 / sum;
                for i in 0..K {
                    out[i] *= inv;
                }
            } else {
                out.fill(1.0 / K as f32);
            }
        } else {
            out.fill(1.0 / K as f32);
        }
    }

    /// Compute the strategy that will actually be exported for this idx.
    ///
    /// Preferred path: normalize the reach-weighted strategy sum.
    /// Fallback: if no reach has accumulated (the infoset is only
    /// reachable through zero-probability actions of some ancestor),
    /// fall back to the current regret-matched strategy. That strategy
    /// is populated for every visited infoset regardless of reach,
    /// because regret updates are weighted by opponent reach, not own
    /// reach.
    ///
    /// Last resort: uniform, only if regrets are also flat.
    ///
    /// This function is what the exporter, the analysis pass, and the
    /// sampled infosets all call. Reporting and export agree by
    /// construction.
    #[inline]
    fn compute_export_strategy(&self, idx: usize) -> [f32; K] {
        let mut out = [0.0f32; K];

        // Preferred: normalize strategy sum.
        let mut sum = 0.0f64;
        for i in 0..K {
            sum += self.load_sum(idx, i);
        }
        if sum > 0.0 {
            let inv = 1.0 / sum;
            for i in 0..K {
                out[i] = (self.load_sum(idx, i) * inv) as f32;
            }
            return out;
        }

        // Fallback: regret-matched current strategy.
        let mut rsum = 0.0f32;
        for i in 0..K {
            let raw = self.load_rm(idx, i, RM_REGRET) as f32 / SCALE;
            let val = raw.max(0.0);
            out[i] = val;
            rsum += val;
        }
        if rsum > 0.0 {
            let inv = 1.0 / rsum;
            for i in 0..K {
                out[i] *= inv;
            }
            return out;
        }

        // Last resort: uniform.
        out.fill(1.0 / K as f32);
        out
    }

    pub fn get_average_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        let guard = self.hash_to_idx.pin();
        if let Some(idx) = guard.get(&infoset_hash) {
            *out = self.compute_export_strategy(*idx);
            return;
        }
        out.fill(1.0 / K as f32);
    }

    pub fn add_strategy_sum(&self, infoset_hash: u64, action_idx: usize, prob: f32) {
        let idx = self.get_or_create_idx(infoset_hash);
        self.add_strategy_sum_at(idx, action_idx, prob);
    }

    #[inline(always)]
    pub fn add_strategy_sum_at(&self, idx: usize, action_idx: usize, prob: f32) {
        self.add_sum(idx, action_idx, prob as f64);
    }

    /// Apply a batch of deferred strategy updates. Returns the number of
    /// distinct (idx, action) pairs that were actually written.
    ///
    /// Sort-dedup: par_sort by (idx, action), walk contiguous groups,
    /// parallel-write each group. After dedup every key is unique, so no
    /// two parallel groups touch the same atomic cell.
    pub fn apply_strategy_batch(&self, ops: &mut Vec<StrategyOp>) -> u64 {
        ops.retain(|op| op.prob != 0.0);
        if ops.is_empty() {
            return 0;
        }
        ops.par_sort_unstable_by_key(|op| (op.index, op.action));

        let mut groups: Vec<(usize, usize, u32, u8)> = Vec::new();
        let mut i = 0usize;
        while i < ops.len() {
            let (idx, act) = (ops[i].index, ops[i].action);
            let mut end = i + 1;
            while end < ops.len() && ops[end].index == idx && ops[end].action == act {
                end += 1;
            }
            groups.push((i, end, idx, act));
            i = end;
        }

        let applied = groups.len() as u64;
        let ops_ref: &[StrategyOp] = ops.as_slice();
        let n_threads = rayon::current_num_threads().max(1);
        let chunk_size = (groups.len() / n_threads).max(1);
        groups.par_chunks(chunk_size).for_each(|grp_slice| {
            for &(start, end, idx_u32, act_u8) in grp_slice {
                let mut prob = 0.0f64;
                for k in start..end {
                    prob += ops_ref[k].prob as f64;
                }
                self.add_sum(idx_u32 as usize, act_u8 as usize, prob);
            }
        });
        applied
    }

    /// Apply a batch of deferred regret updates. Returns (input_len,
    /// unique_count).
    pub fn flush_cpu_batch(&self, batch: &mut Vec<BatchItem>) -> (u64, u64) {
        let input_len = batch.len() as u64;
        if batch.is_empty() {
            return (0, 0);
        }
        batch.par_sort_unstable_by_key(|item| (item.index, item.action));
        let iteration = batch[0].iteration;

        let mut groups: Vec<(usize, usize, u32, u32)> = Vec::new();
        let mut i = 0usize;
        while i < batch.len() {
            let (idx, act) = (batch[i].index, batch[i].action);
            let mut end = i + 1;
            while end < batch.len() && batch[end].index == idx && batch[end].action == act {
                end += 1;
            }
            groups.push((i, end, idx, act));
            i = end;
        }

        let unique_len = groups.len() as u64;
        let batch_ref: &[BatchItem] = batch.as_slice();
        let n_threads = rayon::current_num_threads().max(1);
        let chunk_size = (groups.len() / n_threads).max(1);
        groups.par_chunks(chunk_size).for_each(|grp_slice| {
            for &(start, end, idx_u32, act_u32) in grp_slice {
                let mut delta = 0.0f32;
                for k in start..end {
                    delta += batch_ref[k].delta;
                }
                let idx = idx_u32 as usize;
                let a = act_u32 as usize;
                // T1.1: exact integer discount. cur/mom are raw fixed-point
                // i32 at SCALE; delta is f32 chips, converted to fixed-point.
                let cur_i64 = self.load_rm(idx, a, RM_REGRET) as i64;
                let mom_i64 = self.load_rm(idx, a, RM_MOMENTUM) as i64;
                let delta_i64 = (delta as f64 * SCALE as f64).round() as i64;
                let (new_r, new_m) =
                    crate::dcfr::update_regret_i64(cur_i64, mom_i64, iteration, delta_i64);
                // Clamp to i32 range for storage; the accumulator is i64
                // across updates but the on-disk representation stays i32.
                let r32 = new_r.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                let m32 = new_m.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                self.store_rm(idx, a, RM_REGRET, r32);
                self.store_rm(idx, a, RM_MOMENTUM, m32);
            }
        });
        (input_len, unique_len)
    }

    pub fn flush_gpu_batch(&self, batch: &[BatchItem]) {
        let mut dedup_map: HashMap<(u32, u32), f32> =
            HashMap::with_capacity(batch.len().min(100_000));
        let mut iteration = 0u32;
        for item in batch {
            let key = (item.index, item.action);
            let entry = dedup_map.entry(key).or_insert(0.0);
            *entry += item.delta;
            iteration = item.iteration;
        }
        let deduped: Vec<BatchItem> = dedup_map
            .into_iter()
            .map(|((index, action), delta)| BatchItem {
                index,
                action,
                iteration,
                delta,
            })
            .collect();

        let gpu = self.gpu.get_or_init(|| GpuState::new(self.capacity));
        let max = gpu.max_batch_size();
        for chunk in deduped.chunks(max) {
            let results = gpu.flush_batch(chunk);
            for (item, result) in chunk.iter().zip(results.iter()) {
                let idx = item.index as usize;
                let a = item.action as usize;
                self.store_rm(idx, a, RM_REGRET, result.regret);
                self.store_rm(idx, a, RM_MOMENTUM, result.momentum);
            }
        }
    }

    /// Raw i32 regret for an action at a known idx. Used by FBRS pruning
    /// (Brown & Sandholm, NeurIPS 2015) to decide when to skip exploring
    /// a hopeless action. Cheap inline read.
    #[inline(always)]
    pub fn regret_scaled(&self, idx: usize, action: usize) -> i32 {
        self.load_rm(idx, action, RM_REGRET)
    }

    pub fn get_regret(&self, infoset_hash: u64, action_idx: usize) -> f32 {
        let guard = self.hash_to_idx.pin();
        guard
            .get(&infoset_hash)
            .map(|idx| self.load_rm(*idx, action_idx, RM_REGRET) as f32 / SCALE)
            .unwrap_or(0.0)
    }

    pub fn get_keys(&self) -> Vec<u64> {
        let guard = self.hash_to_idx.pin();
        guard.iter().map(|(k, _)| *k).collect()
    }

    pub fn get_average_strategy_slice(&self, infoset_hash: u64) -> Option<[f32; K]> {
        let guard = self.hash_to_idx.pin();
        guard.get(&infoset_hash).map(|idx| {
            let idx = *idx;
            let mut out = [0.0f32; K];
            for i in 0..K {
                out[i] = self.load_sum(idx, i) as f32;
            }
            out
        })
    }

    pub fn snapshot(&self) -> TableSnapshot {
        let n = self.next_idx.load(Ordering::Relaxed).min(self.capacity);
        let mut max_abs = 0.0f32;
        let mut sum_abs = 0.0f64;
        let mut nonfinite = 0usize;
        let mut strat_mass = 0.0f64;
        let entries_rm = n * RM_STRIDE;
        for i in 0..entries_rm {
            let v = self.data[i].load(Ordering::Relaxed) as f32 / SCALE;
            if !v.is_finite() {
                nonfinite += 1;
                continue;
            }
            let a = v.abs();
            if a > max_abs {
                max_abs = a;
            }
            sum_abs += a as f64;
        }
        let entries_sum = n * SUM_STRIDE;
        for i in 0..entries_sum {
            strat_mass += f64::from_bits(self.strategy_sum[i].load(Ordering::Relaxed));
        }
        let guard = self.hash_to_idx.pin();
        let infosets = guard.len();
        drop(guard);
        TableSnapshot {
            infosets,
            capacity: self.capacity,
            max_abs_regret: max_abs,
            mean_abs_regret: if entries_rm > 0 {
                (sum_abs / entries_rm as f64) as f32
            } else {
                0.0
            },
            nonfinite_count: nonfinite,
            strategy_sum_mass: strat_mass,
        }
    }

    pub fn analyze_strategies(&self) -> StrategyAnalysis {
        let guard = self.hash_to_idx.pin();
        let mut analysis = StrategyAnalysis {
            total: 0,
            empty: 0,
            pure: 0,
            mixed: 0,
            mean_entropy: 0.0,
            entropy_histogram: [0usize; 8],
            dominant_counts: [0usize; K],
            nonzero_strategy_sum_cells: 0,
            uniform_fallback: 0,
        };
        let mut entropy_sum = 0.0f64;
        let mut visited = 0usize;

        for (_, &idx) in guard.iter() {
            analysis.total += 1;
            // Count nonzero cells for the diagnostic.
            let mut sum_raw = 0.0f64;
            for a in 0..K {
                let v = self.load_sum(idx, a);
                if v > 0.0 {
                    analysis.nonzero_strategy_sum_cells += 1;
                }
                sum_raw += v;
            }
            if sum_raw <= 0.0 {
                analysis.empty += 1;
            }

            // The strategy that will actually be exported for this
            // infoset. Uses the same fallback chain as the exporter.
            let strat = self.compute_export_strategy(idx);
            visited += 1;

            // Classify: uniform when all six equal (the last-resort case,
            // where even regrets were flat). Counted separately because
            // these are the infosets where we genuinely have no signal.
            let mut is_uniform = true;
            for a in 1..K {
                if (strat[a] - strat[0]).abs() > 1e-6 {
                    is_uniform = false;
                    break;
                }
            }
            if is_uniform {
                analysis.uniform_fallback += 1;
            }

            let mut best_a = 0usize;
            let mut best_p = 0.0f32;
            for a in 0..K {
                if strat[a] > best_p {
                    best_p = strat[a];
                    best_a = a;
                }
            }
            analysis.dominant_counts[best_a] += 1;

            if best_p >= 0.99 {
                analysis.pure += 1;
            }
            let mut above_tenth = 0usize;
            for a in 0..K {
                if strat[a] >= 0.10 {
                    above_tenth += 1;
                }
            }
            if above_tenth >= 2 {
                analysis.mixed += 1;
            }

            let mut h = 0.0f64;
            for a in 0..K {
                let p = strat[a] as f64;
                if p > 0.0 {
                    h -= p * p.log2();
                }
            }
            entropy_sum += h;
            let bucket = ((h / 0.25) as usize).min(7);
            analysis.entropy_histogram[bucket] += 1;
        }

        if visited > 0 {
            analysis.mean_entropy = entropy_sum / visited as f64;
        }
        analysis
    }

    pub fn sample_infosets(&self, n: usize) -> Vec<InfoSetDump> {
        let guard = self.hash_to_idx.pin();
        let total = guard.len();
        if total == 0 || n == 0 {
            return Vec::new();
        }
        let stride = (total / n).max(1);
        let mut out = Vec::with_capacity(n);
        for (i, (hash, &idx)) in guard.iter().enumerate() {
            if i % stride != 0 {
                continue;
            }
            if out.len() >= n {
                break;
            }
            let strategy = self.compute_export_strategy(idx);
            let mut regrets = [0.0f32; K];
            for a in 0..K {
                regrets[a] = self.load_rm(idx, a, RM_REGRET) as f32 / SCALE;
            }
            out.push(InfoSetDump {
                hash: *hash,
                strategy,
                regrets,
            });
        }
        out
    }

    pub fn hash_contains(&self, infoset_hash: u64) -> bool {
        let guard = self.hash_to_idx.pin();
        guard.contains_key(&infoset_hash)
    }

    pub fn len(&self) -> usize {
        let guard = self.hash_to_idx.pin();
        guard.len()
    }

    pub fn is_empty(&self) -> bool {
        let guard = self.hash_to_idx.pin();
        guard.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn save_checkpoint(&self, path: &str, iteration: u32) -> std::io::Result<()> {
        use std::io::{BufWriter, Write};
        let f = std::fs::File::create(path)?;
        let mut w = BufWriter::with_capacity(1 << 20, f);
        let n = self.next_idx.load(Ordering::Relaxed).min(self.capacity);
        w.write_all(b"PKRCKPT4")?;
        w.write_all(&4u32.to_le_bytes())?;
        w.write_all(&(K as u32).to_le_bytes())?;
        w.write_all(&iteration.to_le_bytes())?;
        w.write_all(&(n as u64).to_le_bytes())?;
        let guard = self.hash_to_idx.pin();
        let map_len = guard.len() as u64;
        w.write_all(&map_len.to_le_bytes())?;
        for (k, v) in guard.iter() {
            w.write_all(&k.to_le_bytes())?;
            w.write_all(&(*v as u64).to_le_bytes())?;
        }
        let rm_entries = n * RM_STRIDE;
        for i in 0..rm_entries {
            w.write_all(&self.data[i].load(Ordering::Relaxed).to_le_bytes())?;
        }
        let sum_entries = n * SUM_STRIDE;
        for i in 0..sum_entries {
            w.write_all(&self.strategy_sum[i].load(Ordering::Relaxed).to_le_bytes())?;
        }
        w.flush()?;
        Ok(())
    }

    pub fn load_checkpoint(&self, path: &str) -> std::io::Result<u32> {
        use std::io::Read;
        let mut f = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        let mut p = 0usize;
        let read = |p: &mut usize, n: usize| -> std::io::Result<&[u8]> {
            if *p + n > buf.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "checkpoint truncated",
                ));
            }
            let s = &buf[*p..*p + n];
            *p += n;
            Ok(s)
        };
        let magic = read(&mut p, 8)?;
        if magic != b"PKRCKPT4" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad checkpoint magic (expected v4 format)",
            ));
        }
        let version = u32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
        if version != 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unsupported checkpoint version",
            ));
        }
        let k = u32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap()) as usize;
        if k != K {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "checkpoint K mismatch",
            ));
        }
        let iteration = u32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
        let n = u64::from_le_bytes(read(&mut p, 8)?.try_into().unwrap()) as usize;
        if n > self.capacity {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "checkpoint larger than table capacity",
            ));
        }
        let map_len = u64::from_le_bytes(read(&mut p, 8)?.try_into().unwrap()) as usize;
        let guard = self.hash_to_idx.pin();
        guard.clear();
        for _ in 0..map_len {
            let key = u64::from_le_bytes(read(&mut p, 8)?.try_into().unwrap());
            let idx = u64::from_le_bytes(read(&mut p, 8)?.try_into().unwrap()) as usize;
            if idx >= self.capacity {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "checkpoint index out of range",
                ));
            }
            guard.insert(key, idx);
        }
        let rm_entries = n * RM_STRIDE;
        for i in 0..rm_entries {
            let v = i32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
            self.data[i].store(v, Ordering::Relaxed);
        }
        let sum_entries = n * SUM_STRIDE;
        for i in 0..sum_entries {
            let v = u64::from_le_bytes(read(&mut p, 8)?.try_into().unwrap());
            self.strategy_sum[i].store(v, Ordering::Relaxed);
        }
        self.next_idx.store(n, Ordering::Relaxed);
        IDX_CACHE.with(|c| c.borrow_mut().clear());
        Ok(iteration)
    }
}

#[cfg(test)]
#[cfg(feature = "gpu")]
mod tests {
    use super::*;

    #[test]
    fn flush_writes_back_only_touched_entries_and_is_idempotent_for_untouched() {
        let table = CompactRegretTable::with_capacity(4096);
        let i1 = table.get_or_create_idx(0xDEAD_0001);
        let i2 = table.get_or_create_idx(0xDEAD_0002);

        let mut batch = vec![
            BatchItem {
                index: i1 as u32,
                action: 0,
                iteration: 1,
                delta: 1.5,
            },
            BatchItem {
                index: i1 as u32,
                action: 0,
                iteration: 1,
                delta: 0.5,
            },
            BatchItem {
                index: i2 as u32,
                action: 3,
                iteration: 1,
                delta: -0.25,
            },
        ];
        table.flush_gpu_batch(&batch);

        let expected_regret = 2.0 / std::f32::consts::SQRT_2;
        let actual_regret = table.get_regret(0xDEAD_0001, 0);
        assert!((actual_regret - expected_regret).abs() < 0.01);
        assert!(table.get_regret(0xDEAD_0002, 3) <= 0.01);
        assert_eq!(table.get_regret(0xDEAD_0002, 0), 0.0);

        table.flush_gpu_batch(&batch);
        assert!((table.get_regret(0xDEAD_0001, 0) - expected_regret).abs() < 0.01);
        batch.clear();
        table.flush_gpu_batch(&batch);
    }

    #[test]
    fn cpu_flush_sort_dedup_equals_sum() {
        let table = CompactRegretTable::with_capacity(4096);
        let i1 = table.get_or_create_idx(0xBEEF_0001);
        let i2 = table.get_or_create_idx(0xBEEF_0002);

        let mut batch = vec![
            BatchItem {
                index: i1 as u32,
                action: 0,
                iteration: 1,
                delta: 0.1,
            },
            BatchItem {
                index: i2 as u32,
                action: 2,
                iteration: 1,
                delta: 0.2,
            },
            BatchItem {
                index: i1 as u32,
                action: 0,
                iteration: 1,
                delta: 0.3,
            },
            BatchItem {
                index: i1 as u32,
                action: 0,
                iteration: 1,
                delta: 0.4,
            },
            BatchItem {
                index: i2 as u32,
                action: 2,
                iteration: 1,
                delta: 0.5,
            },
        ];
        let (input, unique) = table.flush_cpu_batch(&mut batch);
        assert_eq!(input, 5);
        assert_eq!(unique, 2);

        let gamma = 1.0 / std::f32::consts::SQRT_2;
        let expected_i1 = gamma * 0.8;
        let expected_i2 = gamma * 0.7;
        assert!((table.get_regret(0xBEEF_0001, 0) - expected_i1).abs() < 0.01);
        assert!((table.get_regret(0xBEEF_0002, 2) - expected_i2).abs() < 0.01);
    }

    /// Regression for the fixed-point truncation bug: an infoset whose
    /// reach_prob is tiny (deep in the tree) must still accumulate a
    /// non-zero strategy_sum. The old i64 fixed-point accumulator rounded
    /// these contributions to zero, leaving 68% of deep infosets uniform.
    #[test]
    fn deep_reach_prob_contributes_to_strategy_sum() {
        let table = CompactRegretTable::with_capacity(64);
        let deep_hash = 0xDEAD_BEEF_CAFE_1234;
        let idx = table.get_or_create_idx(deep_hash);

        // Simulate a depth-12 infoset: reach_prob = 0.3^12 ≈ 5.3e-7.
        let tiny = (0.3f32).powi(12);
        assert!(
            tiny < 1.0e-3,
            "test setup: tiny must be below fixed-point SCALE"
        );

        let mut ops = vec![];
        for a in 0..K {
            ops.push(StrategyOp {
                index: idx as u32,
                action: a as u8,
                prob: tiny * 0.5,
            });
        }
        table.apply_strategy_batch(&mut ops);

        let mut avg = [0.0f32; K];
        table.get_average_strategy_into(deep_hash, &mut avg);
        // With f64 accumulation, the tiny contribution is preserved and
        // the average normalizes to 1/K across the 6 actions.
        let expected = 1.0 / K as f32;
        for a in 0..K {
            assert!(
                (avg[a] - expected).abs() < 1e-5,
                "action {}: expected {}, got {}",
                a,
                expected,
                avg[a]
            );
        }
    }
}
