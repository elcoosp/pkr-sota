use dashmap::DashMap;
use std::sync::atomic::{AtomicI32, Ordering, AtomicUsize};
use crate::dcfr;

const K: usize = 6;
pub(crate) const SCALE: f32 = 1000.0;

pub struct CompactRegretTable {
    hash_to_idx: DashMap<u64, usize>,
    idx_to_hash: Vec<u64>,
    regrets: Vec<AtomicI32>,
    strategy_sum: Vec<AtomicI32>,
    next_idx: AtomicUsize,
    capacity: usize,
}

impl CompactRegretTable {
    pub fn new() -> Self {
        let capacity = 10_000_000;
        let mut regrets = Vec::with_capacity(capacity * K);
        let mut strategy_sum = Vec::with_capacity(capacity * K);
        regrets.resize_with(capacity * K, || AtomicI32::new(0));
        strategy_sum.resize_with(capacity * K, || AtomicI32::new(0));
        Self {
            hash_to_idx: DashMap::new(),
            idx_to_hash: Vec::new(),
            regrets,
            strategy_sum,
            next_idx: AtomicUsize::new(0),
            capacity,
        }
    }

    fn get_or_create_idx(&self, hash: u64) -> usize {
        if let Some(existing) = self.hash_to_idx.get(&hash) {
            return *existing;
        }
        let idx = self.next_idx.fetch_add(1, Ordering::Relaxed);
        if idx >= self.capacity {
            panic!("Flat table capacity exceeded");
        }
        self.hash_to_idx.insert(hash, idx);
        idx
    }

    pub fn get_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        if let Some(idx) = self.hash_to_idx.get(&infoset_hash) {
            let base = *idx * K;
            let mut sum = 0.0f32;
            for i in 0..K {
                let raw = self.regrets[base + i].load(Ordering::Relaxed);
                let val = ((raw as f32) / SCALE).max(0.0);
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

    pub fn get_average_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        if let Some(idx) = self.hash_to_idx.get(&infoset_hash) {
            let base = *idx * K;
            let sum: f32 = (0..K).map(|i| self.strategy_sum[base + i].load(Ordering::Relaxed) as f32).sum();
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

    pub fn apply_regret_batch(&self, batch: &[(u64, usize, u32, f32)]) {
        // Ensure all hashes have indices
        for &(hash, _, _, _) in batch {
            self.get_or_create_idx(hash);
        }
        // Process each update using direct indexing
        for &(hash, action, iteration, delta) in batch {
            let idx = *self.hash_to_idx.get(&hash).unwrap(); // this is a Ref<usize> -> deref to usize
            let base = idx * K;
            let atom = &self.regrets[base + action];
            loop {
                let cur_i = atom.load(Ordering::Relaxed);
                let cur_f = cur_i as f32 / SCALE;
                let (new_f, _) = dcfr::update_regret_pfr_plus(cur_f, 0.0, iteration, delta);
                let new_i = (new_f * SCALE) as i32;
                if atom.compare_exchange_weak(cur_i, new_i, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                    break;
                }
            }
        }
    }

    pub fn get_regret(&self, infoset_hash: u64, action_idx: usize) -> f32 {
        self.hash_to_idx.get(&infoset_hash)
            .map(|idx| self.regrets[*idx * K + action_idx].load(Ordering::Relaxed) as f32 / SCALE)
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
}
