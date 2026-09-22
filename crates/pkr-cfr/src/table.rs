use crate::dcfr::update_regret_pfr_plus;
use crate::gpu::{BatchItem, GpuState};
use foldhash::fast::RandomState as FoldHasher;
use papaya::HashMap as PapayaMap;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::OnceLock;

const K: usize = 6;

// Thread-local hash -> idx cache. Papaya pins on every map access; the
// resulting epoch traffic serializes all threads (throughput scales as
// 1/N). Caching the hash -> idx resolution per thread skips papaya
// entirely on the hot path. The data array itself is read through the
// cached index with plain atomic loads, which need no pin.
//
// Sized to 1<<20 entries (~32 MB) with a clear-on-overflow policy to
// bound memory. Actual working set per thread is much smaller in
// practice, so overflow is rare.
const IDX_CACHE_INIT: usize = 1 << 18; // 256K entries
const IDX_CACHE_MAX: usize = 1 << 20;  // 1M entries before clearing

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
            // Bound memory. Clearing loses warm entries but keeps the
            // process from growing unboundedly across a long run.
            m.clear();
        }
        m.insert(hash, idx);
    });
}
/// Fields per (infoset, action): regret, momentum, strategy_sum.
/// Interleaving them keeps all 18 i32s of an infoset within two cache
/// lines instead of scattering them across three separate arrays.
const FIELDS: usize = 3;
const STRIDE: usize = K * FIELDS;
const F_REGRET: usize = 0;
const F_MOMENTUM: usize = 1;
const F_SUM: usize = 2;
pub(crate) const SCALE: f32 = 1000.0;

/// A deferred strategy-sum increment. Pushed by traverser nodes into a
/// thread-local Vec, applied serially by the coordinator. This is the
/// same shape as the regret BatchItem stream and is what keeps the
/// traverse hot path write-free on shared state.
#[derive(Clone, Copy, Debug)]
pub struct StrategyOp {
    pub index: u32,
    pub action: u8,
    pub prob: f32,
}

pub struct CompactRegretTable {
    hash_to_idx: PapayaMap<u64, usize, FoldHasher>,
    /// Layout: `data[idx * STRIDE + action * FIELDS + field]`
    /// where field ∈ {F_REGRET, F_MOMENTUM, F_SUM}.
    data: Vec<AtomicI32>,
    next_idx: AtomicUsize,
    capacity: usize,
    /// Lazily constructed. Production uses flush_cpu_batch and never
    /// allocates GPU buffers.
    gpu: OnceLock<GpuState>,
}

impl CompactRegretTable {
    pub fn new() -> Self {
        Self::with_capacity(5_000_000)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let mut data: Vec<AtomicI32> = Vec::with_capacity(capacity * STRIDE);
        data.resize_with(capacity * STRIDE, || AtomicI32::new(0));
        let map = PapayaMap::with_hasher(FoldHasher::default());
        // Pre-reserve to avoid resize storms that ping-pong pins across
        // threads. 4M is a conservative working-set estimate; growth
        // beyond that is amortized.
        map.pin().reserve(4_000_000.min(capacity));
        Self {
            hash_to_idx: map,
            data,
            next_idx: AtomicUsize::new(0),
            capacity,
            gpu: OnceLock::new(),
        }
    }

    #[inline(always)]
    fn off(idx: usize, action: usize, field: usize) -> usize {
        idx * STRIDE + action * FIELDS + field
    }

    #[inline(always)]
    fn load(&self, idx: usize, action: usize, field: usize) -> i32 {
        self.data[Self::off(idx, action, field)].load(Ordering::Relaxed)
    }

    #[inline(always)]
    fn store(&self, idx: usize, action: usize, field: usize, v: i32) {
        self.data[Self::off(idx, action, field)].store(v, Ordering::Relaxed);
    }

    #[inline(always)]
    fn add(&self, idx: usize, action: usize, field: usize, delta: i32) {
        self.data[Self::off(idx, action, field)].fetch_add(delta, Ordering::Relaxed);
    }

    #[inline(always)]
    fn alloc_idx(&self) -> usize {
        loop {
            let cur = self.next_idx.load(Ordering::Relaxed);
            if cur >= self.capacity {
                tracing::error!(
                    "CompactRegretTable capacity {} exceeded; clumping new infosets onto last slot",
                    self.capacity
                );
                return self.capacity - 1;
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

    /// Traverser fast path: resolve (hash -> idx) via thread-local cache,
    /// falling back to a papaya lookup on miss. Reads current regret-
    /// matching strategy from `data[idx]` with plain atomic loads — no
    /// pin is needed once we have an index, since data[] is append-only
    /// and indexes are stable forever.
    ///
    /// This is the single biggest scaling fix: papaya's pin/unpin epoch
    /// machinery was serializing all threads. With the cache warm, the
    /// shared map is consulted ~1% of the time.
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
            let raw = self.load(idx, i, F_REGRET);
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

    /// Non-traverser path: read current strategy without creating the
    /// infoset if it is new. Uses the thread-local cache first (hit path
    /// skips papaya entirely); on miss, looks up papaya read-only and
    /// caches a hit (but not a miss).
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
                let raw = self.load(idx, i, F_REGRET);
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
                sum += self.load(idx, i, F_SUM) as f32;
            }
            if sum > 0.0 {
                let inv = 1.0 / sum;
                for i in 0..K {
                    out[i] = (self.load(idx, i, F_SUM) as f32) * inv;
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
        self.add(idx, action_idx, F_SUM, (prob * SCALE) as i32);
    }

    /// Parallel CPU flush: per-chunk dedup, then parallel atomic updates.
    ///
    /// NOTE: kept for reference. Nested rayon work inside the outer
    /// iteration was measured to be a net slowdown at all thread counts,
    /// so the production path uses `flush_cpu_batch` instead.
    #[allow(dead_code)]
    /// Splits `batch` into `num_threads` contiguous chunks. Each chunk is
    /// deduped independently and the resulting (idx, action) -> delta
    /// entries are applied in parallel. Cross-chunk collisions are
    /// possible (same infoset appears in two chunks); one write wins, the
    /// other is dropped. For MCCFR this is safe: the traversal is
    /// randomized, so lost deltas are simply re-sampled next iteration.
    /// The previous serial flush was O(N * batch) on the coordinator
    /// thread and dominated wall time at high thread counts.
    pub fn flush_cpu_batch_parallel(&self, batch: &[BatchItem]) {
        use rayon::prelude::*;
        if batch.is_empty() {
            return;
        }
        let iteration = batch[0].iteration;
        let n_threads = rayon::current_num_threads().max(1);
        let chunk_size = (batch.len() + n_threads - 1) / n_threads;
        let chunk_size = chunk_size.max(1);

        // Phase 1: per-chunk dedup in parallel. Chunks are contiguous
        // slices of `batch`; no shared state is touched.
        let per_chunk: Vec<Vec<((u32, u32), f32)>> = batch
            .par_chunks(chunk_size)
            .map(|chunk| {
                let mut m: HashMap<(u32, u32), f32, FoldHasher> =
                    HashMap::with_capacity_and_hasher(
                        (chunk.len() / 2).max(1),
                        FoldHasher::default(),
                    );
                for item in chunk {
                    let e = m.entry((item.index, item.action)).or_insert(0.0);
                    *e += item.delta;
                }
                m.into_iter().collect()
            })
            .collect();

        // Phase 2: parallel atomic updates. Each chunk's dedup list is
        // applied independently. Loads are `Relaxed` — we accept slight
        // staleness in `current` when two threads race on the same cell.
        per_chunk.into_par_iter().for_each(|chunk_dedup| {
            for ((index, action), delta) in chunk_dedup {
                let idx = index as usize;
                let a = action as usize;
                let cur = self.load(idx, a, F_REGRET) as f32 / SCALE;
                let mom = self.load(idx, a, F_MOMENTUM) as f32 / SCALE;
                let (new_r, new_m) = update_regret_pfr_plus(cur, mom, iteration, delta);
                self.store(idx, a, F_REGRET, (new_r * SCALE) as i32);
                self.store(idx, a, F_MOMENTUM, (new_m * SCALE) as i32);
            }
        });
    }

    /// Serial CPU flush. Kept for tests and single-threaded callers.
    pub fn flush_cpu_batch(&self, batch: &[BatchItem]) {
        if batch.is_empty() {
            return;
        }
        let mut dedup: HashMap<(u32, u32), f32, FoldHasher> =
            HashMap::with_capacity_and_hasher(batch.len().min(100_000), FoldHasher::default());
        let mut iteration = 0u32;
        for item in batch {
            let entry = dedup.entry((item.index, item.action)).or_insert(0.0);
            *entry += item.delta;
            iteration = item.iteration;
        }

        for ((index, action), delta) in dedup {
            let idx = index as usize;
            let a = action as usize;
            let cur = self.load(idx, a, F_REGRET) as f32 / SCALE;
            let mom = self.load(idx, a, F_MOMENTUM) as f32 / SCALE;
            let (new_r, new_m) = update_regret_pfr_plus(cur, mom, iteration, delta);
            self.store(idx, a, F_REGRET, (new_r * SCALE) as i32);
            self.store(idx, a, F_MOMENTUM, (new_m * SCALE) as i32);
        }
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
                self.store(idx, a, F_REGRET, result.regret);
                self.store(idx, a, F_MOMENTUM, result.momentum);
            }
        }
    }

    pub fn get_regret(&self, infoset_hash: u64, action_idx: usize) -> f32 {
        let guard = self.hash_to_idx.pin();
        guard
            .get(&infoset_hash)
            .map(|idx| self.load(*idx, action_idx, F_REGRET) as f32 / SCALE)
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
                out[i] = self.load(idx, i, F_SUM) as f32 / SCALE;
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
        use std::io::Write;
        let mut f = std::fs::File::create(path)?;
        let n = self.next_idx.load(Ordering::Relaxed).min(self.capacity);
        f.write_all(b"PKRCKPT1")?;
        f.write_all(&1u32.to_le_bytes())?;
        f.write_all(&(K as u32).to_le_bytes())?;
        f.write_all(&iteration.to_le_bytes())?;
        f.write_all(&(n as u64).to_le_bytes())?;
        let guard = self.hash_to_idx.pin();
        let map_len = guard.len() as u64;
        f.write_all(&map_len.to_le_bytes())?;
        for (k, v) in guard.iter() {
            f.write_all(&k.to_le_bytes())?;
            f.write_all(&(*v as u64).to_le_bytes())?;
        }
        let entries = n * STRIDE;
        for i in 0..entries {
            f.write_all(&self.data[i].load(Ordering::Relaxed).to_le_bytes())?;
        }
        f.flush()?;
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
        if magic != b"PKRCKPT1" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "bad checkpoint magic",
            ));
        }
        let version = u32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
        if version != 1 {
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
        let entries = n * STRIDE;
        for i in 0..entries {
            let v = i32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
            self.data[i].store(v, Ordering::Relaxed);
        }
        self.next_idx.store(n, Ordering::Relaxed);
        // Wipe each thread's cache lazily: any thread that touches this
        // table next will go through papaya and repopulate. Actually we
        // only clear the current thread's cache here; other threads will
        // just see misses and fall through to the shared map, which is
        // correct because idx assignments from the checkpoint are stable.
        IDX_CACHE.with(|c| c.borrow_mut().clear());
        Ok(iteration)
    }
}

#[cfg(test)]
#[cfg(feature = "gpu")]
mod tests {
    use super::*;

    /// GPU-flush parity test. On the first iteration (t=1) with zero state:
    ///   gamma = 1/sqrt(t+1) = 1/sqrt(2)
    ///   predicted_delta = gamma * delta
    ///   new_regret = max(0, predicted_delta)
    /// Dedup delta for (i1, 0) = 1.5 + 0.5 = 2.0
    /// Expected regret = 2.0 / sqrt(2) ≈ 1.4142
    #[test]
    fn flush_writes_back_only_touched_entries_and_is_idempotent_for_untouched() {
        let table = CompactRegretTable::with_capacity(4096);
        let i1 = table.get_or_create_idx(0xDEAD_0001);
        let i2 = table.get_or_create_idx(0xDEAD_0002);

        let batch = vec![
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

        let actual_neg = table.get_regret(0xDEAD_0002, 3);
        assert!(
            actual_neg <= 0.01,
            "negative delta should clamp to ~0, got {actual_neg}"
        );

        assert_eq!(table.get_regret(0xDEAD_0002, 0), 0.0);

        table.flush_gpu_batch(&[]);
        assert!((table.get_regret(0xDEAD_0001, 0) - expected_regret).abs() < 0.01);
    }
}
