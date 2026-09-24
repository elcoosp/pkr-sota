// T1.1: flush_cpu_batch now uses the integer path
// (crate::dcfr::update_regret_i64). The f32 wrapper is kept for A/B
// comparison and tests.
//
// GPU path (feature = "gpu") is i32-only and unmaintained; it does NOT
// mirror the i64 table. `flush_gpu_batch` is feature-gated and its tests
// only run under `--features gpu`. Production never calls it.

use crate::gpu::BatchItem;
#[cfg(feature = "gpu")]
use crate::gpu::GpuState;
use crate::metrics::LocalMetrics;
use foldhash::fast::RandomState as FoldHasher;
use papaya::HashMap as PapayaMap;
use rayon::prelude::*;
use std::cell::RefCell;
use std::sync::atomic::{AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

const K: usize = 6;
/// Regret + momentum interleaved: [r0 m0 r1 m1 r2 m2 r3 m3 r4 m4 r5 m5]
/// Written only by the coordinator (flush_cpu_batch), so no false sharing.
const RM_FIELDS: usize = 2;
const RM_STRIDE: usize = K * RM_FIELDS;
const RM_REGRET: usize = 0;
const RM_MOMENTUM: usize = 1;

/// How `flush_cpu_batch` folds deltas. Read once from the environment.
///   PKR_F5_SEQUENTIAL=0  → batched-sum fold (v9..v16 behaviour)
///   PKR_MOMENTUM=0       → plain CFR+/DCFR, no PCFR+ momentum term
/// Defaults reproduce the current production behaviour.
#[derive(Clone, Copy, Debug)]
pub struct FlushMode {
    pub sequential: bool,
    pub momentum: bool,
}

impl FlushMode {
    pub fn from_env() -> Self {
        let off = |n: &str| {
            matches!(
                std::env::var(n).as_deref(),
                Ok("0") | Ok("off") | Ok("false")
            )
        };
        Self {
            sequential: !off("PKR_F5_SEQUENTIAL"),
            momentum: !off("PKR_MOMENTUM"),
        }
    }
    pub fn production() -> Self {
        static M: OnceLock<FlushMode> = OnceLock::new();
        *M.get_or_init(Self::from_env)
    }
}

#[inline]
fn to_fixed(x: f64) -> i64 {
    if x.is_finite() {
        (x * SCALE as f64).round() as i64
    } else {
        0
    }
}

/// Strategy sums live in a separate array indexed [s0..s5].
/// Stored as f64 bits in AtomicU64 — see `add_sum` for why fixed-point was
/// wrong here: reach_prob decays multiplicatively through the tree, and at
/// depth ~5 it drops below 1e-3. A fixed-point i64 with SCALE=1000 truncates
/// those contributions to zero, silently leaving 68% of deep infosets with
/// uniform strategies in the exported blueprint. f64 has no such floor.
const SUM_STRIDE: usize = K;

pub(crate) const SCALE: f32 = 1000.0;

/// Maximum |regret| / |momentum| stored in the i32 fixed-point tables,
/// expressed at `SCALE`. Clipping below `i32::MAX` leaves headroom for
/// the next batch's delta and prevents the saturation pathology that
/// silently uniformizes regret-matching on high-traffic infosets.
///
/// 500_000 / SCALE=1000 = 500 chips = 2.5× starting stack.
/// Any strategy preference stronger than that is indistinguishable in
/// practice, so clipping there costs nothing.
pub(crate) const R_MAX: i64 = i64::MAX / 4;

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

// P1-d: direct-mapped per-thread idx cache. Replaces the HashMap that
// had a hard clear-at-1M-entries cliff (hit rate collapsed above 1M
// infosets). Direct mapping means colliding keys evict one slot; the
// hash is uniform, so eviction is cheap. Memory: 2^19 * 16 B = 8 MB
// per thread.
const CACHE_BITS: u32 = 19;
const CACHE_SIZE: usize = 1 << CACHE_BITS;

#[derive(Clone, Copy)]
struct CacheSlot {
    key: u64,
    idx: u32,
}

thread_local! {
    static IDX_CACHE: RefCell<Vec<CacheSlot>> = RefCell::new(vec![
        CacheSlot { key: 0, idx: 0 };
        CACHE_SIZE
    ]);
}

// Generation counter for the thread-local IDX_CACHE (audit F14):
// bumped on every checkpoint load so entries cached before the load
// can never alias post-load indices on a worker thread that did not
// observe the clear. Folded into the key derivation, so old entries
// simply miss.
static CACHE_GEN: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
fn cache_key(hash: u64) -> u64 {
    hash ^ CACHE_GEN
        .load(Ordering::Relaxed)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

#[inline(always)]
fn cache_slot(key: u64) -> usize {
    (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - CACHE_BITS)) as usize
}

#[inline]
fn cache_lookup(hash: u64) -> Option<usize> {
    let key = cache_key(hash);
    // key==0 cannot be a valid entry (0 is the sentinel for an empty slot);
    // the probability of a genuine FNV-1a-collide-with-sentinel is 2^-64,
    // so we treat it as a miss.
    if key == 0 {
        return None;
    }
    IDX_CACHE.with(|c| {
        let slots = c.borrow();
        let s = &slots[cache_slot(key)];
        if s.key == key {
            Some(s.idx as usize)
        } else {
            None
        }
    })
}

#[inline]
fn cache_insert(hash: u64, idx: usize) {
    let key = cache_key(hash);
    if key == 0 {
        // Extremely rare: skip insert rather than clobber the empty sentinel.
        return;
    }
    IDX_CACHE.with(|c| {
        let mut slots = c.borrow_mut();
        let s = &mut slots[cache_slot(key)];
        s.key = key;
        s.idx = idx as u32;
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
    data: Vec<AtomicI64>,
    /// f64 strategy sums stored as u64 bits. Independent array to avoid
    /// false sharing with the interleaved regret/momentum data.
    strategy_sum: Vec<AtomicU64>,
    next_idx: AtomicUsize,
    capacity: usize,
    #[cfg(feature = "gpu")]
    gpu: OnceLock<GpuState>,
}

impl Default for CompactRegretTable {
    fn default() -> Self {
        Self::new()
    }
}

impl CompactRegretTable {
    /// Regret-matching+ strategy from the stored regrets. Since the
    /// normalisation cancels SCALE, we skip the /SCALE division entirely
    /// (audit E2). Uniform over all K actions if no positive regret.
    #[inline(always)]
    fn regret_match_into(&self, idx: usize, out: &mut [f32; K]) {
        let mut sum = 0.0f32;
        for i in 0..K {
            let raw = self.load_rm(idx, i, RM_REGRET);
            let v = if raw > 0 { raw as f32 } else { 0.0 };
            out[i] = v;
            sum += v;
        }
        if sum > 0.0 {
            let inv = 1.0 / sum;
            for i in 0..K {
                out[i] *= inv;
            }
        } else {
            out.fill(1.0 / K as f32);
        }
    }

    pub fn new() -> Self {
        Self::with_capacity(5_000_000)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let mut data: Vec<AtomicI64> = Vec::with_capacity(capacity * RM_STRIDE);
        data.resize_with(capacity * RM_STRIDE, || AtomicI64::new(0));
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
            #[cfg(feature = "gpu")]
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
    fn load_rm(&self, idx: usize, action: usize, field: usize) -> i64 {
        self.data[Self::off_rm(idx, action, field)].load(Ordering::Relaxed)
    }

    #[inline(always)]
    fn store_rm(&self, idx: usize, action: usize, field: usize, v: i64) {
        self.data[Self::off_rm(idx, action, field)].store(v, Ordering::Relaxed);
    }

    #[inline(always)]
    fn load_sum(&self, idx: usize, action: usize) -> f64 {
        f64::from_bits(self.strategy_sum[Self::off_sum(idx, action)].load(Ordering::Relaxed))
    }

    /// CAS-loop add. Used by the public `add_strategy_sum_at`, which may
    /// be called concurrently from multiple threads on the same cell.
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

    /// Non-CAS add for the batch path: after `apply_strategy_batch`'s
    /// sort+dedup, each (idx, action) appears in exactly one parallel
    /// group, so only one thread ever touches a given cell. Load+store
    /// avoids the CAS loop's branch and memory ordering fence.
    #[inline(always)]
    fn add_sum_grouped(&self, idx: usize, action: usize, delta: f64) {
        let cell = &self.strategy_sum[Self::off_sum(idx, action)];
        let cur = f64::from_bits(cell.load(Ordering::Relaxed));
        cell.store((cur + delta).to_bits(), Ordering::Relaxed);
    }

    /// Number of slots handed out (>= distinct infosets: lost races leak slots).
    #[inline]
    pub fn allocated(&self) -> usize {
        self.next_idx.load(Ordering::Relaxed).min(self.capacity)
    }

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

    #[inline]
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
        self.regret_match_into(idx, out);
        idx
    }

    #[inline]
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
            self.regret_match_into(idx, out);
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

        // Fallback: regret-matched current strategy (or uniform).
        self.regret_match_into(idx, &mut out);
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
        // E4b: bit-identical grouping key, single u64 compare (audit E4).
        ops.par_sort_unstable_by_key(|op| ((op.index as u64) << 8) | op.action as u64);

        let mut groups: Vec<(usize, usize, u32, u8)> = Vec::with_capacity(ops.len() / 4 + 16);
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
        // E4a: match flush_cpu_batch's chunking (audit section E4).
        let chunk_size = (groups.len() / (n_threads * 8)).max(64);
        groups.par_chunks(chunk_size).for_each(|grp_slice| {
            for &(start, end, idx_u32, act_u8) in grp_slice {
                let mut prob = 0.0f64;
                for k in start..end {
                    prob += ops_ref[k].prob as f64;
                }
                self.add_sum_grouped(idx_u32 as usize, act_u8 as usize, prob);
            }
        });
        applied
    }

    /// Apply a batch of deferred regret updates. Returns (input_len,
    /// unique_count).
    /// Apply a batch of deferred regret updates. Returns (input_len, unique_count).
    /// Uses `FlushMode::production()` (env read once).
    pub fn flush_cpu_batch(&self, batch: &mut Vec<BatchItem>) -> (u64, u64) {
        self.flush_cpu_batch_with(batch, FlushMode::production())
    }

    /// Same as `flush_cpu_batch` but with explicit mode. Tests should use
    /// this; production goes through `flush_cpu_batch` -> `FlushMode::production`.
    pub fn flush_cpu_batch_with(&self, batch: &mut Vec<BatchItem>, mode: FlushMode) -> (u64, u64) {
        let input_len = batch.len() as u64;
        if batch.is_empty() {
            return (0, 0);
        }
        batch.par_sort_unstable_by_key(|item| (item.index, item.action, item.iteration));

        let mut groups: Vec<(usize, usize, u32, u32)> = Vec::with_capacity(batch.len() / 4 + 16);
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
        // B3 audit: smaller chunks -> more parallel groups -> better work
        // stealing on skewed group-size distributions.
        let chunk_size = (groups.len() / (n_threads * 8)).max(64);

        groups.par_chunks(chunk_size).for_each(|grp_slice| {
            for &(start, end, idx_u32, act_u32) in grp_slice {
                let idx = idx_u32 as usize;
                let a = act_u32 as usize;
                let mut cur_i64 = self.load_rm(idx, a, RM_REGRET);
                let mut mom_i64 = self.load_rm(idx, a, RM_MOMENTUM);

                if mode.sequential {
                    for k in start..end {
                        let delta_i64 = to_fixed(batch_ref[k].delta as f64);
                        let (new_r, new_m) = crate::dcfr::update_regret_i64_mode(
                            cur_i64,
                            mom_i64,
                            batch_ref[k].iteration,
                            delta_i64,
                            mode.momentum,
                        );
                        if new_r == i64::MAX {
                            warn_nonfinite_regret_once(batch_ref[k].iteration);
                        }
                        cur_i64 = new_r;
                        mom_i64 = new_m;
                    }
                } else {
                    // Batched-sum fold: non-finite deltas skipped (B1/B3).
                    let mut delta_sum: f64 = 0.0;
                    let mut max_iter: u32 = batch_ref[start].iteration;
                    for k in start..end {
                        let d = batch_ref[k].delta;
                        if d.is_finite() {
                            delta_sum += d as f64;
                        }
                        if batch_ref[k].iteration > max_iter {
                            max_iter = batch_ref[k].iteration;
                        }
                    }
                    let delta_i64 = to_fixed(delta_sum);
                    let (new_r, new_m) = crate::dcfr::update_regret_i64_mode(
                        cur_i64,
                        mom_i64,
                        max_iter,
                        delta_i64,
                        mode.momentum,
                    );
                    if new_r == i64::MAX {
                        warn_nonfinite_regret_once(max_iter);
                    }
                    cur_i64 = new_r;
                    mom_i64 = new_m;
                }

                // Clip to R_MAX for arithmetic headroom; not a CFR clip.
                let r64 = cur_i64.clamp(-R_MAX, R_MAX);
                let m64 = mom_i64.clamp(-R_MAX, R_MAX);
                self.store_rm(idx, a, RM_REGRET, r64);
                self.store_rm(idx, a, RM_MOMENTUM, m64);
            }
        });
        (input_len, unique_len)
    }

    #[cfg(feature = "gpu")]
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
                self.store_rm(idx, a, RM_REGRET, result.regret as i64);
                self.store_rm(idx, a, RM_MOMENTUM, result.momentum as i64);
            }
        }
    }

    /// Raw i32 regret for an action at a known idx. Used by FBRS pruning
    /// (Brown & Sandholm, NeurIPS 2015) to decide when to skip exploring
    /// a hopeless action. Cheap inline read.
    #[inline(always)]
    pub fn regret_scaled(&self, idx: usize, action: usize) -> i64 {
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
        // A5/B8: regret-only snapshot. Previously this loop also walked
        // the momentum cells, conflating "regret magnitude" with the
        // PCFR+ prediction EMA.
        let n = self.allocated();
        let mut max_abs = 0.0f32;
        let mut sum_abs = 0.0f64;
        let mut nonfinite = 0usize;
        for idx in 0..n {
            for a in 0..K {
                let v = self.load_rm(idx, a, RM_REGRET) as f32 / SCALE;
                if !v.is_finite() {
                    nonfinite += 1;
                    continue;
                }
                let av = v.abs();
                if av > max_abs {
                    max_abs = av;
                }
                sum_abs += av as f64;
            }
        }
        let mut strat_mass = 0.0f64;
        let entries_sum = n * SUM_STRIDE;
        for i in 0..entries_sum {
            strat_mass += f64::from_bits(self.strategy_sum[i].load(Ordering::Relaxed));
        }
        // B4: allocated() reflects true slot consumption (races leak slots
        // that len() would not count); this is what is_near_capacity uses.
        let infosets = self.allocated();
        TableSnapshot {
            infosets,
            capacity: self.capacity,
            max_abs_regret: max_abs,
            mean_abs_regret: if n > 0 {
                (sum_abs / (n * K) as f64) as f32
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

    pub fn save_checkpoint(
        &self,
        path: &str,
        iteration: u32,
        fingerprint: &pkr_core::abstraction::AbstractionFingerprint,
    ) -> std::io::Result<()> {
        use std::io::{BufWriter, Write};
        let f = std::fs::File::create(path)?;
        let mut w = BufWriter::with_capacity(1 << 20, f);
        let n = self.allocated();
        w.write_all(b"PKRCKPT7")?;
        w.write_all(&7u32.to_le_bytes())?;
        w.write_all(bytemuck::bytes_of(fingerprint))?;
        w.write_all(&(K as u32).to_le_bytes())?;
        w.write_all(&iteration.to_le_bytes())?;
        w.write_all(&(n as u64).to_le_bytes())?;
        let guard = self.hash_to_idx.pin();
        let map_len = guard.len() as u64;
        w.write_all(&map_len.to_le_bytes())?;
        // Pack 16 bytes per write_all to halve the call-count on the
        // 5M-key path (each write_all re-checks BufWriter capacity).
        let mut kv_buf = [0u8; 16];
        for (k, v) in guard.iter() {
            kv_buf[0..8].copy_from_slice(&k.to_le_bytes());
            kv_buf[8..16].copy_from_slice(&(*v as u64).to_le_bytes());
            w.write_all(&kv_buf)?;
        }
        // P2-a: stream the backing arrays directly. AtomicI64/AtomicU64
        // have the same size/alignment as i64/u64 (std guarantee) and
        // every bit pattern is valid; we only take a shared view and
        // elements are never mutated through it.
        let rm_entries = n * RM_STRIDE;
        let rm: &[i64] =
            unsafe { std::slice::from_raw_parts(self.data.as_ptr() as *const i64, rm_entries) };
        let rm_bytes: &[u8] =
            unsafe { std::slice::from_raw_parts(rm.as_ptr() as *const u8, rm_entries * 8) };
        w.write_all(rm_bytes)?;

        let sum_entries = n * SUM_STRIDE;
        let sums: &[u64] = unsafe {
            std::slice::from_raw_parts(self.strategy_sum.as_ptr() as *const u64, sum_entries)
        };
        let sum_bytes: &[u8] =
            unsafe { std::slice::from_raw_parts(sums.as_ptr() as *const u8, sum_entries * 8) };
        w.write_all(sum_bytes)?;
        w.flush()?;
        Ok(())
    }

    pub fn load_checkpoint(
        &self,
        path: &str,
        current: &pkr_core::abstraction::AbstractionFingerprint,
    ) -> std::io::Result<u32> {
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
        if magic == b"PKRCKPT6" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "checkpoint is v6 (i64 regret cells but pre-A2 layout). \
                 Start a fresh run or pass --fresh to discard.",
            ));
        }
        if magic != b"PKRCKPT7" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad checkpoint magic (expected v5 format)",
            ));
        }
        let version = u32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
        if version != 7 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unsupported checkpoint version",
            ));
        }
        // F2b: read the stored fingerprint and compare to the current
        // configuration. A mismatch means the checkpoint was written
        // by a binary whose abstraction differed in a way that changes
        // infoset identity or action meaning. Resuming would silently
        // corrupt training (r3 rule 0.1).
        let fp_bytes = read(&mut p, 40)?;
        let stored_fp: &pkr_core::abstraction::AbstractionFingerprint =
            bytemuck::from_bytes(fp_bytes);
        if stored_fp != current {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "checkpoint abstraction mismatch: {}. \
                     Delete the checkpoint and restart, or resume with the \
                     original binary.",
                    stored_fp.describe_mismatch(current),
                ),
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
        // Reset ALL state first, so a failed/partial load cannot leave
        // stale cells that `alloc_idx` would later hand out as fresh.
        self.data
            .par_iter()
            .for_each(|c| c.store(0, Ordering::Relaxed));
        self.strategy_sum
            .par_iter()
            .for_each(|c| c.store(0, Ordering::Relaxed));
        self.next_idx.store(0, Ordering::Relaxed);
        IDX_CACHE.with(|c| *c.borrow_mut() = vec![CacheSlot { key: 0, idx: 0 }; CACHE_SIZE]);
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
        // P2-a: bulk read + per-element atomic store. The slow part was
        // calling `read()` once per element (each call re-checks bounds
        // and slices buf); doing one big read and iterating the slice is
        // 10-50x faster at 5M infosets. Atomics still need per-cell
        // stores because we hold `&self`.
        let rm_entries = n * RM_STRIDE;
        let rm_bytes = read(&mut p, rm_entries * 8)?;
        for (i, chunk) in rm_bytes.as_chunks::<8>().0.iter().enumerate() {
            let v = i64::from_le_bytes(*chunk);
            self.data[i].store(v, Ordering::Relaxed);
        }
        let sum_entries = n * SUM_STRIDE;
        let sum_bytes = read(&mut p, sum_entries * 8)?;
        for (i, chunk) in sum_bytes.as_chunks::<8>().0.iter().enumerate() {
            let v = u64::from_le_bytes(*chunk);
            self.strategy_sum[i].store(v, Ordering::Relaxed);
        }
        self.next_idx.store(n, Ordering::Relaxed);
        CACHE_GEN.fetch_add(1, Ordering::Relaxed);
        IDX_CACHE.with(|c| *c.borrow_mut() = vec![CacheSlot { key: 0, idx: 0 }; CACHE_SIZE]);
        Ok(iteration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "gpu")]
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

        // Sequential per-iteration PCFR+ fold (audit F5) at t=1 (warmup,
        // discount=1, gamma=1/sqrt(2)): each delta folds with its own
        // momentum state rather than collapsing to one update.
        //   i1: 100 -> r=71; +300 -> r=304; +400 -> r=655  => 0.655
        //   i2: 200 -> r=141; +500 -> r=536                => 0.536
        assert!((table.get_regret(0xBEEF_0001, 0) - 0.655).abs() < 0.01);
        assert!((table.get_regret(0xBEEF_0002, 2) - 0.536).abs() < 0.01);
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

#[cfg(test)]
mod f5_tests {
    use super::*;

    #[test]
    fn flush_folds_deltas_sequentially_per_iteration() {
        // Only valid when the sequential fold is enabled via env var.
        if std::env::var("PKR_F5_SEQUENTIAL").as_deref() != Ok("1") {
            eprintln!("SKIP: PKR_F5_SEQUENTIAL not set");
            return;
        }
        let table = CompactRegretTable::with_capacity(4096);
        let i1 = table.get_or_create_idx(0xCAFE_0001);
        // r=0; iter1 delta=+10 (t=1 < TAU -> no discount); iter2 delta=-6.
        //
        // Sequential PCFR+ on the i64 fixed-point path (SCALE=1000,
        // gamma = 1/sqrt(t+1)):
        //   t=1: pred = round(0.7071*10000) = 7071 -> r = 7071, m = 7071
        //   t=2: pred = round(0.4226*7071 + 0.5774*(-6000)) = -476
        //        r = max(0, 7071 - 476) = 6595  => 6.595
        // Collapsed (old code): one update with delta_sum = +4:
        //   pred = round(0.7071*4000) = 2828 -> r = 2.828
        // The sequential result must win. This is the clamp-semantics
        // regression the audit (F5) is about.
        let mut batch = vec![
            BatchItem {
                index: i1 as u32,
                action: 0,
                iteration: 1,
                delta: 10.0,
            },
            BatchItem {
                index: i1 as u32,
                action: 0,
                iteration: 2,
                delta: -6.0,
            },
        ];
        table.flush_cpu_batch(&mut batch);
        let r = table.get_regret(0xCAFE_0001, 0);
        assert!(
            (r - 6.595).abs() < 0.05,
            "expected ~6.595 (sequential fold), got {r}"
        );
    }
}

#[cfg(test)]
mod ckpt_v7_tests {
    use super::*;
    use pkr_core::abstraction::AbstractionFingerprint;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pkr_{}_{}.ckpt", name, std::process::id()))
    }

    #[test]
    fn ckpt_v7_roundtrip_beyond_i32() {
        let fp = AbstractionFingerprint::from_constants(4);
        let a = CompactRegretTable::with_capacity(64);
        let idx = a.get_or_create_idx(0xABCD);
        a.store_rm(idx, 2, RM_REGRET, 5_000_000_000_000i64);
        a.add_strategy_sum_at(idx, 1, 0.25);
        let p = tmp("v7rt");
        a.save_checkpoint(p.to_str().unwrap(), 42, &fp).unwrap();

        let b = CompactRegretTable::with_capacity(64);
        assert_eq!(b.load_checkpoint(p.to_str().unwrap(), &fp).unwrap(), 42);
        let j = b.get_or_create_idx(0xABCD);
        assert_eq!(b.regret_scaled(j, 2), 5_000_000_000_000i64);
        assert!((b.load_sum(j, 1) - 0.25).abs() < 1e-12);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn ckpt_v6_rejected() {
        let fp = AbstractionFingerprint::from_constants(4);
        let p = tmp("v6rej");
        std::fs::write(&p, b"PKRCKPT6\x06\x00\x00\x00").unwrap();
        let t = CompactRegretTable::with_capacity(8);
        let e = t.load_checkpoint(p.to_str().unwrap(), &fp).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
        let _ = std::fs::remove_file(p);
    }
}

/// Post-audit regression tests. Each test names the audit item it protects.
#[cfg(test)]
mod audit_regression_tests {
    use super::*;

    /// B4: `allocated()` reflects slot consumption, not just map len().
    #[test]
    fn allocated_tracks_slot_consumption() {
        let t = CompactRegretTable::with_capacity(64);
        assert_eq!(t.allocated(), 0);
        let i1 = t.get_or_create_idx(0xA1_0001);
        let i2 = t.get_or_create_idx(0xA1_0002);
        let i3 = t.get_or_create_idx(0xA1_0003);
        assert_ne!(i1, i2);
        assert_ne!(i2, i3);
        assert_ne!(i1, i3);
        assert_eq!(t.allocated(), 3);
        // Second call with the same hash must NOT consume another slot.
        let again = t.get_or_create_idx(0xA1_0001);
        assert_eq!(again, i1);
        assert_eq!(t.allocated(), 3);
    }

    /// B4 + A5: the snapshot reports `allocated()`, which is what
    /// `is_near_capacity` also reads.
    #[test]
    fn snapshot_infosets_matches_allocated() {
        let t = CompactRegretTable::with_capacity(64);
        for k in 0..10u64 {
            t.get_or_create_idx(0xB4_0000 + k);
        }
        let snap = t.snapshot();
        assert_eq!(snap.infosets, t.allocated());
        assert_eq!(snap.infosets, 10);
    }

    /// B2 invariant: after any flush, stored regret is never negative.
    /// This is what made FBRS pruning dead code; the test guards against
    /// reintroducing a negative-regret path.
    #[test]
    fn regret_is_never_negative_after_flush() {
        let t = CompactRegretTable::with_capacity(64);
        let idx = t.get_or_create_idx(0xB2_0000);
        let mut b = vec![
            BatchItem {
                index: idx as u32,
                action: 0,
                iteration: 1,
                delta: -1.0e6,
            },
            BatchItem {
                index: idx as u32,
                action: 1,
                iteration: 1,
                delta: -1.0e9,
            },
            BatchItem {
                index: idx as u32,
                action: 2,
                iteration: 1,
                delta: 5.0,
            },
        ];
        t.flush_cpu_batch_with(
            &mut b,
            FlushMode {
                sequential: true,
                momentum: false,
            },
        );
        for a in 0..K {
            let r = t.load_rm(idx, a, RM_REGRET);
            assert!(r >= 0, "regret[action {a}] = {r} < 0");
        }
    }

    /// B1 companion: a NaN delta in sequential mode is treated as 0 by
    /// `to_fixed`, not silently corrupting the stored regret. (The
    /// batched-mode NaN test lives in `i64_tests`.)
    #[test]
    fn nan_delta_in_sequential_flush_is_zero() {
        let t = CompactRegretTable::with_capacity(16);
        let idx = t.get_or_create_idx(0xB1_0000);
        let mut b = vec![
            BatchItem {
                index: idx as u32,
                action: 0,
                iteration: 1,
                delta: 1.0,
            },
            BatchItem {
                index: idx as u32,
                action: 0,
                iteration: 1,
                delta: f32::NAN,
            },
        ];
        t.flush_cpu_batch_with(
            &mut b,
            FlushMode {
                sequential: true,
                momentum: false,
            },
        );
        let r = t.load_rm(idx, 0, RM_REGRET);
        assert!(!(r as f32).is_nan());
        // SCALE = 1000; one 1.0 delta, then a 0 delta.
        assert_eq!(r, 1000);
    }

    /// B3 sanity: both fold modes apply the same *sum* of deltas for a
    /// single-action group with equal-iteration entries, so switching
    /// modes does not silently drop updates.
    #[test]
    fn sequential_and_batched_agree_when_iterations_match() {
        let mk_batch = |idx: usize| {
            vec![
                BatchItem {
                    index: idx as u32,
                    action: 0,
                    iteration: 1,
                    delta: 3.0,
                },
                BatchItem {
                    index: idx as u32,
                    action: 0,
                    iteration: 1,
                    delta: 4.0,
                },
            ]
        };

        let t1 = CompactRegretTable::with_capacity(16);
        let i1 = t1.get_or_create_idx(0xB3_0000);
        let mut b1 = mk_batch(i1);
        t1.flush_cpu_batch_with(
            &mut b1,
            FlushMode {
                sequential: true,
                momentum: false,
            },
        );

        let t2 = CompactRegretTable::with_capacity(16);
        let i2 = t2.get_or_create_idx(0xB3_0000);
        let mut b2 = mk_batch(i2);
        t2.flush_cpu_batch_with(
            &mut b2,
            FlushMode {
                sequential: false,
                momentum: false,
            },
        );

        let r1 = t1.load_rm(i1, 0, RM_REGRET);
        let r2 = t2.load_rm(i2, 0, RM_REGRET);
        assert_eq!(r1, r2);
        assert_eq!(r1, 7000); // (3+4) chips * SCALE
    }

    /// E4b: the strategy-batch sort key ((index << 8) | action) groups
    /// ops with the same (index, action) together. Two identical op
    /// entries must sum, not race.
    #[test]
    fn strategy_batch_groups_by_index_and_action() {
        let t = CompactRegretTable::with_capacity(16);
        let idx = t.get_or_create_idx(0xE4_B000);
        let mut ops = vec![
            StrategyOp {
                index: idx as u32,
                action: 0,
                prob: 0.25,
            },
            StrategyOp {
                index: idx as u32,
                action: 0,
                prob: 0.25,
            },
            StrategyOp {
                index: idx as u32,
                action: 1,
                prob: 0.50,
            },
            StrategyOp {
                index: idx as u32,
                action: 0,
                prob: 0.25,
            },
        ];
        let applied = t.apply_strategy_batch(&mut ops);
        assert_eq!(applied, 2, "two distinct (index, action) groups expected");
        assert!(
            (t.load_sum(idx, 0) - 0.75).abs() < 1e-9,
            "action 0 sum = {}",
            t.load_sum(idx, 0)
        );
        assert!(
            (t.load_sum(idx, 1) - 0.50).abs() < 1e-9,
            "action 1 sum = {}",
            t.load_sum(idx, 1)
        );
    }

    /// A5: the snapshot's `max_abs_regret` is regret-only. Writing a huge
    /// momentum value must NOT appear in the snapshot's regret magnitude.
    #[test]
    fn snapshot_regret_only_ignores_momentum_cells() {
        let t = CompactRegretTable::with_capacity(16);
        let idx = t.get_or_create_idx(0xA5_0000);
        // Set regret to a small positive value, momentum to something huge.
        t.store_rm(idx, 0, RM_REGRET, 1000); // 1 chip
        t.store_rm(idx, 0, RM_MOMENTUM, 100_000_000); // 100k chips
        let snap = t.snapshot();
        // max_abs_regret is reported in CHIPS (units / SCALE), so 1.0.
        assert!(
            snap.max_abs_regret <= 1.5,
            "max_abs_regret = {} (momentum leaked in?)",
            snap.max_abs_regret
        );
    }
}
