use crate::dcfr;
use dashmap::DashMap;
use std::sync::atomic::{AtomicI32, Ordering};

const K: usize = 6;
pub(crate) const SCALE: f32 = 1000.0;

// Align to 64 bytes to perfectly fit one M1 cache line.
// This eliminates L2/L3 cache thrashing.
#[repr(align(64))]
struct InfosetNode {
    regret: [AtomicI32; K],
    momentum: [AtomicI32; K],
    strategy_sum: [AtomicI32; K],
}

impl InfosetNode {
    fn new() -> Self {
        Self {
            regret: [const { AtomicI32::new(0) }; K],
            momentum: [const { AtomicI32::new(0) }; K],
            strategy_sum: [const { AtomicI32::new(0) }; K],
        }
    }
}

pub struct CompactRegretTable {
    map: DashMap<u64, Box<InfosetNode>>,
}

impl CompactRegretTable {
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    pub fn get_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        if let Some(node) = self.map.get(&infoset_hash) {
            let mut sum = 0.0f32;
            for i in 0..K {
                let raw = node.regret[i].load(Ordering::Relaxed);
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
        if let Some(node) = self.map.get(&infoset_hash) {
            let sum: f32 = (0..K)
                .map(|i| node.strategy_sum[i].load(Ordering::Relaxed) as f32)
                .sum();
            if sum > 0.0 {
                let inv = 1.0 / sum;
                for i in 0..K {
                    out[i] = (node.strategy_sum[i].load(Ordering::Relaxed) as f32) * inv;
                }
                return;
            }
        }
        out.fill(1.0 / K as f32);
    }

    pub fn add_strategy_sum(&self, infoset_hash: u64, action_idx: usize, prob: f32) {
        let node = self
            .map
            .entry(infoset_hash)
            .or_insert_with(|| Box::new(InfosetNode::new()));
        node.strategy_sum[action_idx].fetch_add((prob * SCALE) as i32, Ordering::Relaxed);
    }

    pub fn apply_regret_batch(&self, batch: &[(u64, usize, u32, f32)]) {
        for &(hash, action, iteration, delta) in batch {
            let node = self
                .map
                .entry(hash)
                .or_insert_with(|| Box::new(InfosetNode::new()));
            let atom_reg = &node.regret[action];
            let atom_mom = &node.momentum[action];

            loop {
                let cur_i = atom_reg.load(Ordering::Relaxed);
                let mom_i = atom_mom.load(Ordering::Relaxed);

                let cur_f = cur_i as f32 / SCALE;
                let mom_f = mom_i as f32 / SCALE;

                let (new_f, new_mom_f) =
                    dcfr::update_regret_pfr_plus(cur_f, mom_f, iteration, delta);

                let new_i = (new_f * SCALE) as i32;
                let new_mom_i = (new_mom_f * SCALE) as i32;

                if atom_reg
                    .compare_exchange_weak(cur_i, new_i, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                {
                    atom_mom.store(new_mom_i, Ordering::Relaxed);
                    break;
                }
            }
        }
    }

    pub fn get_regret(&self, infoset_hash: u64, action_idx: usize) -> f32 {
        self.map
            .get(&infoset_hash)
            .map(|node| node.regret[action_idx].load(Ordering::Relaxed) as f32 / SCALE)
            .unwrap_or(0.0)
    }

    pub fn get_keys(&self) -> Vec<u64> {
        self.map.iter().map(|e| *e.key()).collect()
    }

    pub fn get_average_strategy_slice(&self, infoset_hash: u64) -> Option<[f32; K]> {
        self.map.get(&infoset_hash).map(|node| {
            let mut out = [0.0f32; K];
            for i in 0..K {
                out[i] = node.strategy_sum[i].load(Ordering::Relaxed) as f32 / SCALE;
            }
            out
        })
    }
}
