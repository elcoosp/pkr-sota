use crate::dcfr::update_regret_pfr_plus;
use crate::gpu::{BatchItem, GpuState};
use crate::metrics::LocalMetrics;
use foldhash::fast::RandomState as FoldHasher;
use papaya::HashMap as PapayaMap;
use rayon::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicUsize, Ordering};
use std::sync::OnceLock;

const K: usize = 6;
const RM_FIELDS: usize = 2;
const RM_STRIDE: usize = K * RM_FIELDS;
const RM_REGRET: usize = 0;
const RM_MOMENTUM: usize = 1;
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
    pub empty: usize,
    pub pure: usize,
    pub mixed: usize,
    pub mean_entropy: f64,
    pub entropy_histogram: [usize; 8],
    pub dominant_counts: [usize; K],
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

fn warn_nonfinite_regret_once(iteration: u32) {
    use std::sync::OnceLock;
    static WARNED: OnceLock<()> = OnceLock::new();
    WARNED.get_or_init(|| {
        eprintln!(
            "WARNING: regret became non-finite at iteration {}. \
             Training is corrupt from this point."
            , iteration
        );
    });
}

pub struct CompactRegretTable {
    hash_to_idx: PapayaMap<u64, usize, FoldHasher>,
    data: Vec<AtomicI32>,
    strategy_sum: Vec<AtomicI64>,
    next_idx: AtomicUsize,
    capacity: usize,
    gpu: OnceLock<GpuState>,
}

impl CompactRegretTable {
    pub fn new() -> Self {
        Self::with_capacity(5_000_000)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let mut data: Vec<AtomicI32> = Vec::with_capacity(capacity * RM_STRIDE);
        data.resize_with(capacity * RM_STRIDE, || AtomicI32::new(0));
        let mut strategy_sum: Vec<AtomicI64> = Vec::with_capacity(capacity * SUM_STRIDE);
        strategy_sum.resize_with(capacity * SUM_STRIDE, || AtomicI64::new(0));
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
    fn load_sum(&self, idx: usize, action: usize) -> i64 {
        self.strategy_sum[Self::off_sum(idx, action)].load(Ordering::Relaxed)
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

    pub fn get_average_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        let guard = self.hash_to_idx.pin();
        if let Some(idx) = guard.get(&infoset_hash) {
            let idx = *idx;
            let mut sum = 0.0f32;
            for i in 0..K {
                sum += self.load_sum(idx, i) as f32;
            }
            if sum > 0.0 {
                let inv = 1.0 / sum;
                for i in 0..K {
                    out[i] = (self.load_sum(idx, i) as f32) * inv;
                }
                return;
            }
        }
        out.fill(1.0 / K as f32);
    }

    pub fn add_strategy_sum(&self, infoset_hash: u64, action_idx: usize, prob: f32) {
        let idx = self.get_or_create_idx(infoset_hash);
        self.add_strategy_sum_at(idx, action_idx, prob);
    }

    #[inline(always)]
    pub fn add_strategy_sum_at(&self, idx: usize, action_idx: usize, prob: f32) {
        let off = Self::off_sum(idx, action_idx);
        self.strategy_sum[off].fetch_add((prob * SCALE) as i64, Ordering::Relaxed);
    }

    /// Apply a batch of deferred strategy updates. Returns the number of
    /// distinct (idx, action) pairs that were actually written.
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
                let mut prob = 0.0f32;
                for k in start..end {
                    prob += ops_ref[k].prob;
                }
                let off = Self::off_sum(idx_u32 as usize, act_u8 as usize);
                self.strategy_sum[off].fetch_add((prob * SCALE) as i64, Ordering::Relaxed);
            }
        });
        applied
    }

    /// Apply a batch of deferred regret updates. Returns (input_len,
    /// unique_count) so callers can compute dedup ratio.
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
                let cur = self.load_rm(idx, a, RM_REGRET) as f32 / SCALE;
                let mom = self.load_rm(idx, a, RM_MOMENTUM) as f32 / SCALE;
                let (new_r, new_m) = update_regret_pfr_plus(cur, mom, iteration, delta);
                if !new_r.is_finite() || !new_m.is_finite() {
                    warn_nonfinite_regret_once(iteration);
                }
                self.store_rm(idx, a, RM_REGRET, (new_r * SCALE) as i32);
                self.store_rm(idx, a, RM_MOMENTUM, (new_m * SCALE) as i32);
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
                out[i] = self.load_sum(idx, i) as f32 / SCALE;
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
            strat_mass += self.strategy_sum[i].load(Ordering::Relaxed) as f64;
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
        };
        let mut entropy_sum = 0.0f64;
        let mut visited = 0usize;

        for (_, &idx) in guard.iter() {
            analysis.total += 1;
            let mut s = [0.0f32; K];
            let mut sum = 0.0f32;
            for a in 0..K {
                s[a] = self.load_sum(idx, a) as f32;
                sum += s[a];
            }
            if sum <= 0.0 {
                analysis.empty += 1;
                continue;
            }
            visited += 1;
            for a in 0..K {
                s[a] /= sum;
            }

            let mut best_a = 0usize;
            let mut best_p = 0.0f32;
            for a in 0..K {
                if s[a] > best_p {
                    best_p = s[a];
                    best_a = a;
                }
            }
            analysis.dominant_counts[best_a] += 1;

            if best_p >= 0.99 {
                analysis.pure += 1;
            }
            let mut above_tenth = 0usize;
            for a in 0..K {
                if s[a] >= 0.10 {
                    above_tenth += 1;
                }
            }
            if above_tenth >= 2 {
                analysis.mixed += 1;
            }

            let mut h = 0.0f64;
            for a in 0..K {
                let p = s[a] as f64;
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
            let mut strategy = [0.0f32; K];
            let mut sum = 0.0f32;
            for a in 0..K {
                strategy[a] = self.load_sum(idx, a) as f32;
                sum += strategy[a];
            }
            if sum > 0.0 {
                for a in 0..K {
                    strategy[a] /= sum;
                }
            } else {
                strategy = [1.0 / K as f32; K];
            }
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
        w.write_all(b"PKRCKPT3")?;
        w.write_all(&3u32.to_le_bytes())?;
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
        if magic != b"PKRCKPT3" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad checkpoint magic (expected v3 format)",
            ));
        }
        let version = u32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
        if version != 3 {
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
            let v = i64::from_le_bytes(read(&mut p, 8)?.try_into().unwrap());
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
        assert!(
            (actual_regret - expected_regret).abs() < 0.01,
            "expected ~{expected_regret}, got {actual_regret}"
        );
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
            BatchItem { index: i1 as u32, action: 0, iteration: 1, delta: 0.1 },
            BatchItem { index: i2 as u32, action: 2, iteration: 1, delta: 0.2 },
            BatchItem { index: i1 as u32, action: 0, iteration: 1, delta: 0.3 },
            BatchItem { index: i1 as u32, action: 0, iteration: 1, delta: 0.4 },
            BatchItem { index: i2 as u32, action: 2, iteration: 1, delta: 0.5 },
        ];
        let (input, unique) = table.flush_cpu_batch(&mut batch);
        assert_eq!(input, 5);
        assert_eq!(unique, 2);

        let gamma = 1.0 / std::f32::consts::SQRT_2;
        let expected_i1 = gamma * 0.8;
        let expected_i2 = gamma * 0.7;
        assert!(
            (table.get_regret(0xBEEF_0001, 0) - expected_i1).abs() < 0.01,
            "i1: expected ~{expected_i1}, got {}",
            table.get_regret(0xBEEF_0001, 0)
        );
        assert!(
            (table.get_regret(0xBEEF_0002, 2) - expected_i2).abs() < 0.01,
            "i2: expected ~{expected_i2}, got {}",
            table.get_regret(0xBEEF_0002, 2)
        );
    }
}
