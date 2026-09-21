use crate::gpu::{BatchItem, GpuState};
use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

const K: usize = 6;
pub(crate) const SCALE: f32 = 1000.0;

pub struct CompactRegretTable {
    hash_to_idx: DashMap<u64, usize>,
    cpu_regrets: Vec<AtomicI32>,
    cpu_momentums: Vec<AtomicI32>,
    strategy_sum: Vec<AtomicI32>,
    next_idx: AtomicUsize,
    capacity: usize,
    gpu: GpuState,
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

        let gpu = GpuState::new(capacity);

        Self {
            hash_to_idx: DashMap::new(),
            cpu_regrets,
            cpu_momentums,
            strategy_sum,
            next_idx: AtomicUsize::new(0),
            capacity,
            gpu,
        }
    }

    pub(crate) fn get_or_create_idx(&self, hash: u64) -> usize {
        let entry = self.hash_to_idx.entry(hash);
        *entry.or_insert_with(|| {
            let idx = self.next_idx.fetch_add(1, Ordering::Relaxed);
            if idx >= self.capacity {
                panic!("Flat table capacity exceeded");
            }
            idx
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
        let base = idx * K;
        self.strategy_sum[base + action_idx].fetch_add((prob * SCALE) as i32, Ordering::Relaxed);
    }

    /// CPU-GPU hybrid: deduplicate batch, dispatch to GPU in chunks,
    /// then write back ONLY the touched entries.
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
        let max = self.gpu.max_batch_size();
        for chunk in deduped.chunks(max) {
            let results = self.gpu.flush_batch(chunk);

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
