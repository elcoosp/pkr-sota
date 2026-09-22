use crate::dcfr::update_regret_pfr_plus;
use crate::gpu::{BatchItem, GpuState};
use dashmap::DashMap;
use foldhash::fast::RandomState as FoldHasher;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::OnceLock;

const K: usize = 6;
pub(crate) const SCALE: f32 = 1000.0;

pub struct CompactRegretTable {
    hash_to_idx: DashMap<u64, usize, FoldHasher>,
    cpu_regrets: Vec<AtomicI32>,
    cpu_momentums: Vec<AtomicI32>,
    strategy_sum: Vec<AtomicI32>,
    next_idx: AtomicUsize,
    capacity: usize,
    /// Lazily constructed. The GPU path is only used by tests; production
    /// runs flush_cpu_batch and never touch this. Eager construction here
    /// used to allocate ~2.4 GB of WGPU storage buffers, which fails
    /// against Limits::downlevel_defaults() at capacity=50M and aborts
    /// the process (panic = "abort").
    gpu: OnceLock<GpuState>,
}

impl CompactRegretTable {
    pub fn new() -> Self {
        Self::with_capacity(5_000_000)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let mut cpu_regrets = Vec::with_capacity(capacity * K);
        let mut cpu_momentums = Vec::with_capacity(capacity * K);
        let mut strategy_sum = Vec::with_capacity(capacity * K);

        cpu_regrets.resize_with(capacity * K, || AtomicI32::new(0));
        cpu_momentums.resize_with(capacity * K, || AtomicI32::new(0));
        strategy_sum.resize_with(capacity * K, || AtomicI32::new(0));

        Self {
            hash_to_idx: DashMap::with_hasher(FoldHasher::default()),
            cpu_regrets,
            cpu_momentums,
            strategy_sum,
            next_idx: AtomicUsize::new(0),
            capacity,
            gpu: OnceLock::new(),
        }
    }

    pub(crate) fn get_or_create_idx(&self, hash: u64) -> usize {
        let entry = self.hash_to_idx.entry(hash);
        *entry.or_insert_with(|| {
            // CAS loop so next_idx never advances past capacity.
            // When the table saturates, new hashes share the last slot.
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
        })
    }

    pub fn get_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        if let Some(idx) = self.hash_to_idx.get(&infoset_hash) {
            let base = *idx * K;
            let mut sum = 0.0f32;
            for i in 0..K {
                let raw = self.cpu_regrets[base + i].load(Ordering::Relaxed);
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
        if let Some(idx) = self.hash_to_idx.get(&infoset_hash) {
            let base = *idx * K;
            let sum: f32 = (0..K)
                .map(|i| self.strategy_sum[base + i].load(Ordering::Relaxed) as f32)
                .sum();
            if sum > 0.0 {
                let inv = 1.0 / sum;
                for i in 0..K {
                    out[i] = (self.strategy_sum[base + i].load(Ordering::Relaxed) as f32) * inv;
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

    /// Fast path used by the traversal: assumes the caller already resolved
    /// `infoset_hash` to `idx` via `get_or_create_idx`. Saves K-1 DashMap
    /// lookups per traverser node.
    #[inline(always)]
    pub fn add_strategy_sum_at(&self, idx: usize, action_idx: usize, prob: f32) {
        let base = idx * K;
        self.strategy_sum[base + action_idx].fetch_add((prob * SCALE) as i32, Ordering::Relaxed);
    }

    /// CPU implementation of the PCFR+ DCFR update, semantically identical
    /// to the GPU shader in gpu.rs. For HU NLHE with K=6 the arithmetic is
    /// trivial; skipping the WGPU submit + sync per iteration removes the
    /// dominant per-iteration overhead.
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
            let flat = index as usize * K + action as usize;
            let cur = self.cpu_regrets[flat].load(Ordering::Relaxed) as f32 / SCALE;
            let mom = self.cpu_momentums[flat].load(Ordering::Relaxed) as f32 / SCALE;
            let (new_r, new_m) = update_regret_pfr_plus(cur, mom, iteration, delta);
            self.cpu_regrets[flat]
                .store((new_r * SCALE) as i32, Ordering::Relaxed);
            self.cpu_momentums[flat]
                .store((new_m * SCALE) as i32, Ordering::Relaxed);
        }
    }

    /// CPU-GPU hybrid: deduplicate batch, dispatch to GPU in chunks,
    /// then write back ONLY the touched entries.
    ///
    /// GpuState is constructed on first use. Production runs should
    /// prefer `flush_cpu_batch`; this path exists for the GPU parity test.
    pub fn flush_gpu_batch(&self, batch: &[BatchItem]) {
        // 1. Deduplicate
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

        // 2. Chunked dispatch — a merged cross-thread batch can exceed max_batch_size
        let gpu = self.gpu.get_or_init(|| GpuState::new(self.capacity));
        let max = gpu.max_batch_size();
        for chunk in deduped.chunks(max) {
            let results = gpu.flush_batch(chunk);

            // 3. Touched-only write-back: O(len(chunk)) instead of O(capacity * K)
            for (item, result) in chunk.iter().zip(results.iter()) {
                let flat = item.index as usize * K + item.action as usize;
                self.cpu_regrets[flat].store(result.regret, Ordering::Relaxed);
                self.cpu_momentums[flat].store(result.momentum, Ordering::Relaxed);
            }
        }
    }

    pub fn get_regret(&self, infoset_hash: u64, action_idx: usize) -> f32 {
        self.hash_to_idx
            .get(&infoset_hash)
            .map(|idx| {
                self.cpu_regrets[*idx * K + action_idx].load(Ordering::Relaxed) as f32 / SCALE
            })
            .unwrap_or(0.0)
    }

    pub fn get_keys(&self) -> Vec<u64> {
        self.hash_to_idx.iter().map(|e| *e.key()).collect()
    }

    pub fn get_average_strategy_slice(&self, infoset_hash: u64) -> Option<[f32; K]> {
        self.hash_to_idx.get(&infoset_hash).map(|idx| {
            let base = *idx * K;
            let mut out = [0.0f32; K];
            for i in 0..K {
                out[i] = self.strategy_sum[base + i].load(Ordering::Relaxed) as f32 / SCALE;
            }
            out
        })
    }

    pub fn hash_contains(&self, infoset_hash: u64) -> bool {
        self.hash_to_idx.contains_key(&infoset_hash)
    }

    pub fn len(&self) -> usize {
        self.hash_to_idx.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hash_to_idx.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Serialize the full table (map + all three atomic arrays) to a
    /// single file. Format is self-describing and forward-extensible via
    /// the version field. Takes an &self snapshot, so it is safe to call
    /// while training threads are idle (e.g. between iterations).
    pub fn save_checkpoint(&self, path: &str, iteration: u32) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(path)?;
        let n = self.next_idx.load(Ordering::Relaxed).min(self.capacity);
        f.write_all(b"PKRCKPT1")?;
        f.write_all(&1u32.to_le_bytes())?;
        f.write_all(&(K as u32).to_le_bytes())?;
        f.write_all(&iteration.to_le_bytes())?;
        f.write_all(&(n as u64).to_le_bytes())?;
        let map_len = self.hash_to_idx.len() as u64;
        f.write_all(&map_len.to_le_bytes())?;
        for e in self.hash_to_idx.iter() {
            f.write_all(&e.key().to_le_bytes())?;
            f.write_all(&(*e.value() as u64).to_le_bytes())?;
        }
        let entries = n * K;
        for i in 0..entries {
            f.write_all(&self.cpu_regrets[i].load(Ordering::Relaxed).to_le_bytes())?;
        }
        for i in 0..entries {
            f.write_all(&self.cpu_momentums[i].load(Ordering::Relaxed).to_le_bytes())?;
        }
        for i in 0..entries {
            f.write_all(&self.strategy_sum[i].load(Ordering::Relaxed).to_le_bytes())?;
        }
        f.flush()?;
        Ok(())
    }

    /// Load a checkpoint written by `save_checkpoint`. Clears the current
    /// table first, so it is safe to call on a freshly-constructed table.
    /// Returns the iteration number stored in the checkpoint.
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
        self.hash_to_idx.clear();
        for _ in 0..map_len {
            let key = u64::from_le_bytes(read(&mut p, 8)?.try_into().unwrap());
            let idx = u64::from_le_bytes(read(&mut p, 8)?.try_into().unwrap()) as usize;
            if idx >= self.capacity {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "checkpoint index out of range",
                ));
            }
            self.hash_to_idx.insert(key, idx);
        }
        let entries = n * K;
        for i in 0..entries {
            let v = i32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
            self.cpu_regrets[i].store(v, Ordering::Relaxed);
        }
        for i in 0..entries {
            let v = i32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
            self.cpu_momentums[i].store(v, Ordering::Relaxed);
        }
        for i in 0..entries {
            let v = i32::from_le_bytes(read(&mut p, 4)?.try_into().unwrap());
            self.strategy_sum[i].store(v, Ordering::Relaxed);
        }
        self.next_idx.store(n, Ordering::Relaxed);
        Ok(iteration)
    }
}

#[cfg(test)]
#[cfg(feature = "gpu")]
mod tests {
    use super::*;

    /// Verify that flush_gpu_batch writes back only touched entries and is
    /// a no-op for untouched entries. The GPU applies PCFR+ DCFR update
    /// (momentum + regret discounting), so on the first iteration (t=1)
    /// with zero-initial GPU state:
    ///   gamma = 1/sqrt(t+1) = 1/sqrt(2)
    ///   predicted_delta = gamma * delta  (momentum=0)
    ///   new_regret = max(0, 0 + predicted_delta) = gamma * delta
    /// Deduplicated delta for (i1, action 0) = 1.5 + 0.5 = 2.0
    /// So regret = 2.0 / sqrt(2) ≈ 1.4142, stored as i32 * SCALE.
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

        // Deduped delta = 2.0; PCFR+ on t=1: regret = 2.0 / sqrt(2) ≈ 1.4142
        let expected_regret = 2.0 / std::f32::consts::SQRT_2;
        let actual_regret = table.get_regret(0xDEAD_0001, 0);
        assert!(
            (actual_regret - expected_regret).abs() < 0.01,
            "expected ~{expected_regret}, got {actual_regret}"
        );

        // delta=-0.25; with r_neg=0, discounted_regret=0, new_regret = max(0+predicted, 0) = 0
        // (negative regret after max(0,...) clamps to 0)
        let actual_neg = table.get_regret(0xDEAD_0002, 3);
        let expected_neg = if 1.0 / std::f32::consts::SQRT_2 * (-0.25) > 0.0 {
            1.0 / std::f32::consts::SQRT_2 * (-0.25)
        } else {
            0.0
        };
        assert!(
            (actual_neg - expected_neg).abs() < 0.01,
            "expected ~{expected_neg}, got {actual_neg}"
        );

        // Untouched entry stays zero
        assert_eq!(table.get_regret(0xDEAD_0002, 0), 0.0);

        // Empty batch -> nothing changes
        table.flush_gpu_batch(&[]);
        assert!((table.get_regret(0xDEAD_0001, 0) - expected_regret).abs() < 0.01);
    }
}
