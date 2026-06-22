use std::collections::HashMap;

const K: usize = 6; // abstract actions

pub struct CompactRegretTable {
    regrets: HashMap<u64, [f32; K]>,
    strategy_sum: HashMap<u64, [f32; K]>,
}

impl CompactRegretTable {
    pub fn new() -> Self {
        Self {
            regrets: HashMap::new(),
            strategy_sum: HashMap::new(),
        }
    }

    /// Write strategy into a provided stack buffer (no allocations).
    pub fn get_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        if let Some(r) = self.regrets.get(&infoset_hash) {
            let mut sum = 0.0f32;
            for i in 0..K {
                out[i] = r[i].max(0.0);
                sum += out[i];
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

    /// Write average strategy into a provided stack buffer.
    pub fn get_average_strategy_into(&self, infoset_hash: u64, out: &mut [f32; K]) {
        if let Some(s) = self.strategy_sum.get(&infoset_hash) {
            let sum: f32 = s.iter().sum();
            if sum > 0.0 {
                let inv = 1.0 / sum;
                for i in 0..K {
                    out[i] = s[i] * inv;
                }
                return;
            }
        }
        out.fill(1.0 / K as f32);
    }

    pub fn set_regret(&mut self, infoset_hash: u64, action_idx: usize, val: f32) {
        let entry = self
            .regrets
            .entry(infoset_hash)
            .or_insert([0.0; K]);
        entry[action_idx] = val;
    }

    pub fn add_strategy_sum(&mut self, infoset_hash: u64, action_idx: usize, prob: f32) {
        let entry = self
            .strategy_sum
            .entry(infoset_hash)
            .or_insert([0.0; K]);
        entry[action_idx] += prob;
    }

    /// Returns reference to average strategy as slice (for export).
    pub fn get_average_strategy_slice(&self, infoset_hash: u64) -> Option<&[f32; K]> {
        self.strategy_sum.get(&infoset_hash)
    }

    pub fn get_regret(&self, infoset_hash: u64, action_idx: usize) -> f32 {
        self.regrets
            .get(&infoset_hash)
            .map_or(0.0, |r| r[action_idx])
    }

    /// Returns all infoset hashes present in the strategy_sum.
    pub fn get_keys(&self) -> Vec<u64> {
        self.strategy_sum.keys().copied().collect()
    }

    /// Merge another table into this one (summing regrets and strategy sums).
    pub fn merge(&mut self, other: &CompactRegretTable) {
        for (&key, arr) in &other.regrets {
            let entry = self.regrets.entry(key).or_insert([0.0; K]);
            for i in 0..K {
                entry[i] += arr[i];
            }
        }
        for (&key, arr) in &other.strategy_sum {
            let entry = self.strategy_sum.entry(key).or_insert([0.0; K]);
            for i in 0..K {
                entry[i] += arr[i];
            }
        }
    }
}
