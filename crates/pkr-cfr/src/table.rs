use dashmap::DashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use crate::dcfr;

const K: usize = 6;
pub(crate) const SCALE: f32 = 1000.0;

// Pack regret and momentum into adjacent atomics for cache locality
pub struct RegretState {
    pub regret: AtomicI32,
    pub momentum: AtomicI32,
}

impl RegretState {
    fn new() -> Self {
        Self {
            regret: AtomicI32::new(0),
            momentum: AtomicI32::new(0),
        }
    }
}

pub struct CompactRegretTable {
    regrets: DashMap<u64, [RegretState; K]>,
    strategy_sum: DashMap<u64, [AtomicI32; K]>,
}

impl CompactRegretTable {
    pub fn new() -> Self {
        Self {
            regrets: DashMap::new(),
            strategy_sum: DashMap::new(),
        }
    }

    /// Writes normalized strategy into a stack buffer.
    pub fn get_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        if let Some(r) = self.regrets.get(&infoset_hash) {
            let mut sum = 0.0f32;
            for i in 0..K {
                let raw = r[i].regret.load(Ordering::Relaxed);
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

    pub fn add_strategy_sum(&self, infoset_hash: u64, action_idx: usize, prob: f32) {
        let entry = self.strategy_sum.entry(infoset_hash).or_insert_with(|| {
            [(); K].map(|_| AtomicI32::new(0))
        });
        entry[action_idx].fetch_add((prob * SCALE) as i32, Ordering::Relaxed);
    }

    /// PCFR+ regret update using CAS loop on regret and storing momentum.
    pub fn apply_regret_update(&self, infoset_hash: u64, action_idx: usize, iteration: u32, delta: f32) {
        let entry = self.regrets.entry(infoset_hash).or_insert_with(|| {
            [(); K].map(|_| RegretState::new())
        });
        let state = &entry[action_idx];

        loop {
            let cur_i = state.regret.load(Ordering::Relaxed);
            let mom_i = state.momentum.load(Ordering::Relaxed);

            let cur_f = cur_i as f32 / SCALE;
            let mom_f = mom_i as f32 / SCALE;

            let (new_f, new_mom_f) = dcfr::update_regret_pfr_plus(cur_f, mom_f, iteration, delta);
            let new_i = (new_f * SCALE) as i32;
            let new_mom_i = (new_mom_f * SCALE) as i32;

            // CAS on regret; if it succeeds, store momentum and break.
            if state.regret.compare_exchange_weak(
                cur_i, new_i, Ordering::Relaxed, Ordering::Relaxed
            ).is_ok() {
                state.momentum.store(new_mom_i, Ordering::Relaxed);
                break;
            }
        }
    }

    pub fn get_regret(&self, infoset_hash: u64, action_idx: usize) -> f32 {
        self.regrets.get(&infoset_hash)
            .map(|r| r[action_idx].regret.load(Ordering::Relaxed) as f32 / SCALE)
            .unwrap_or(0.0)
    }

    pub fn get_keys(&self) -> Vec<u64> {
        self.strategy_sum.iter().map(|e| *e.key()).collect()
    }

    pub fn get_average_strategy_slice(&self, infoset_hash: u64) -> Option<[f32; K]> {
        self.strategy_sum.get(&infoset_hash).map(|arr| {
            let mut out = [0.0f32; K];
            for i in 0..K {
                out[i] = arr[i].load(Ordering::Relaxed) as f32 / SCALE;
            }
            out
        })
    }

    pub fn merge(&self, _other: &CompactRegretTable) {}
}
