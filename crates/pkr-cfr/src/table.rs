use crate::dcfr::update_regret_pfr_plus;
use crate::gpu::{BatchItem, GpuState};
use foldhash::fast::RandomState as FoldHasher;
use papaya::HashMap as PapayaMap;
use rayon::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicUsize, Ordering};
use std::sync::OnceLock;

const K: usize = 6;
/// Regrets and momentums are interleaved: `rm[idx * K*2 + action*2 + field]`
/// where field 0 = regret, 1 = momentum. Safe because only the coordinator
/// writes them (flush_cpu_batch runs single-threaded at the group level).
const RM_FIELDS: usize = 2;
const RM_STRIDE: usize = K * RM_FIELDS;
const RM_REGRET: usize = 0;
const RM_MOMENTUM: usize = 1;

/// Strategy sums are in their own array. They are written atomically by
/// every traverser node from every thread, so keeping them separate from
/// the regret data eliminates false sharing between neighbors.
const SUM_STRIDE: usize = K;

pub(crate) const SCALE: f32 = 1000.0;

/// A deferred strategy-sum increment. Pushed by traverser nodes into a
/// thread-local Vec, applied once per dispatch by the coordinator.
#[derive(Clone, Copy, Debug)]
pub struct StrategyOp {
    pub index: u32,
    pub action: u8,
    pub prob: f32,
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

pub struct CompactRegretTable {
    hash_to_idx: PapayaMap<u64, usize, FoldHasher>,
    /// Interleaved regret+momentum: `data[idx*RM_STRIDE + action*2 + field]`.
    data: Vec<AtomicI32>,
    /// Strategy sums: `strategy_sum[idx*K + action]`. Separate array to
    /// avoid false sharing with the interleaved regret/momentum data.
    /// i64 because i32 fixed-point ×1000 saturates after ~2.1M weighted
    /// visits per slot; hot preflop infosets hit that in a few thousand
    /// iterations.
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

    /// Allocate a fresh slot index. Panics on overflow rather than silently
    /// clumping onto the last slot, which would corrupt training results
    /// without any visible signal. The trainer checks is_near_capacity()
    /// at 95% and stops cleanly before this fires.
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

    pub fn get_strategy_and_idx(&self, infoset_hash: u64, out: &mut [f32; K]) -> usize {
        let idx = match cache_lookup(infoset_hash) {
            Some(i) => i,
            None => {
                let i = self.get_or_create_idx(infoset_hash);
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

    /// Apply a batch of deferred strategy updates.
    ///
    /// Sort-dedup version: filter zero-probability ops (traverser pushes
    /// all K ops including illegal actions), par_sort by (idx, action),
    /// then parallel-walk contiguous groups. After dedup every (idx,
    /// action) is unique, so the atomic fetch_add per group never touches
    /// the same cell twice. Contiguous groups improve cache behaviour
    /// versus random HashMap lookups.
    pub fn apply_strategy_batch(&self, ops: &mut Vec<StrategyOp>) {
        ops.retain(|op| op.prob != 0.0);
        if ops.is_empty() {
            return;
        }
        ops.par_sort_unstable_by_key(|op| (op.index, op.action));

        // Serial scan to build the group index vector. This is O(n) over
        // contiguous memory, cheaper than the equivalent number of hash
        // inserts.
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
    }

    /// Apply a batch of deferred regret updates.
    ///
    /// Same sort-dedup shape as apply_strategy_batch. Rayon's
    /// par_sort_unstable_by_key is faster than building a HashMap, and
    /// the group scan reads contiguously. `update_regret_pfr_plus` is a
    /// read-modify-write, but after dedup every key is unique so no two
    /// parallel groups touch the same cell.
    pub fn flush_cpu_batch(&self, batch: &mut Vec<BatchItem>) {
        if batch.is_empty() {
            return;
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
                self.store_rm(idx, a, RM_REGRET, (new_r * SCALE) as i32);
                self.store_rm(idx, a, RM_MOMENTUM, (new_m * SCALE) as i32);
            }
        });
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

    /// The new sort-dedup CPU flush must produce the same result as a
    /// straight sum-then-update against the same inputs.
    #[test]
    fn cpu_flush_sort_dedup_equals_sum() {
        let table = CompactRegretTable::with_capacity(4096);
        let i1 = table.get_or_create_idx(0xBEEF_0001);
        let i2 = table.get_or_create_idx(0xBEEF_0002);

        // Randomly ordered, multiple deltas per key.
        let mut batch = vec![
            BatchItem { index: i1 as u32, action: 0, iteration: 1, delta: 0.1 },
            BatchItem { index: i2 as u32, action: 2, iteration: 1, delta: 0.2 },
            BatchItem { index: i1 as u32, action: 0, iteration: 1, delta: 0.3 },
            BatchItem { index: i1 as u32, action: 0, iteration: 1, delta: 0.4 },
            BatchItem { index: i2 as u32, action: 2, iteration: 1, delta: 0.5 },
        ];
        table.flush_cpu_batch(&mut batch);

        // delta_i1_a0 = 0.8, delta_i2_a2 = 0.7.
        // On iteration 1 with zero state: gamma = 1/sqrt(2), regret = gamma*delta.
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
