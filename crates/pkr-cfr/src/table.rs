use dashmap::DashMap;
use std::sync::atomic::{AtomicI32, Ordering};

const K: usize = 6;
const SCALE: f32 = 1000.0; // scale factor for i32 regret storage

pub struct CompactRegretTable {
    regrets: DashMap<u64, [AtomicI32; K]>,
    strategy_sum: DashMap<u64, [AtomicI32; K]>, // also quantized
}

impl CompactRegretTable {
    pub fn new() -> Self {
        Self {
            regrets: DashMap::new(),
            strategy_sum: DashMap::new(),
        }
    }

    /// Writes normalized strategy into a stack buffer (no allocations).
    pub fn get_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        if let Some(r) = self.regrets.get(&infoset_hash) {
            let mut sum = 0.0f32;
            for i in 0..K {
                let raw = r[i].load(Ordering::Relaxed);
                let val = (raw as f32).max(0.0);
                out[i] = val;
                sum += val;
            }
            if sum > 0.0 {
                let inv = 1.0 / sum;
                for i in 0..K { out[i] *= inv; }
            } else {
                out.fill(1.0 / K as f32);
            }
        } else {
            out.fill(1.0 / K as f32);
        }
    }

    /// Writes average strategy into a stack buffer.
    pub fn get_average_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        if let Some(s) = self.strategy_sum.get(&infoset_hash) {
            let sum: f32 = s.iter().map(|a| a.load(Ordering::Relaxed) as f32).sum();
            if sum > 0.0 {
                let inv = 1.0 / sum;
                for i in 0..K {
                    out[i] = (s[i].load(Ordering::Relaxed) as f32) * inv;
                }
                return;
            }
        }
        out.fill(1.0 / K as f32);
    }

    /// Set regret (atomic store).
    pub fn set_regret(&self, infoset_hash: u64, action_idx: usize, val: f32) {
        let scaled = (val * SCALE) as i32;
        let entry = self.regrets.entry(infoset_hash).or_insert_with(|| {
            [(); K].map(|_| AtomicI32::new(0))
        });
        entry[action_idx].store(scaled, Ordering::Relaxed);
    }

    /// Add to strategy sum (atomic add).
    pub fn add_strategy_sum(&self, infoset_hash: u64, action_idx: usize, prob: f32) {
        let scaled = (prob * SCALE) as i32;
        let entry = self.strategy_sum.entry(infoset_hash).or_insert_with(|| {
            [(); K].map(|_| AtomicI32::new(0))
        });
        entry[action_idx].fetch_add(scaled, Ordering::Relaxed);
    }

    /// Get regret (f32, unscaled).
    pub fn get_regret(&self, infoset_hash: u64, action_idx: usize) -> f32 {
        self.regrets.get(&infoset_hash)
            .map(|r| r[action_idx].load(Ordering::Relaxed) as f32 / SCALE)
            .unwrap_or(0.0)
    }

    /// Returns all infoset hashes present in strategy_sum.
    pub fn get_keys(&self) -> Vec<u64> {
        self.strategy_sum.iter().map(|e| *e.key()).collect()
    }

    /// Average strategy slice for export (returns None if missing).
    pub fn get_average_strategy_slice(&self, infoset_hash: u64) -> Option<[f32; K]> {
        self.strategy_sum.get(&infoset_hash).map(|arr| {
            let mut out = [0.0f32; K];
            for i in 0..K {
                out[i] = arr[i].load(Ordering::Relaxed) as f32 / SCALE;
            }
            out
        })
    }

    /// Merge is no longer needed because the table is shared lock-free across threads.
    /// This method is kept as a no-op for compatibility but should not be called.
    pub fn merge(&self, _other: &CompactRegretTable) {
        // shared state, nothing to merge
    }
}
