use std::collections::HashMap;

pub struct CompactRegretTable {
    num_actions: usize,
    regrets: HashMap<u64, Vec<f32>>,
    strategy_sum: HashMap<u64, Vec<f32>>,
}

impl CompactRegretTable {
    pub fn new(_capacity: usize, num_actions: usize) -> Self {
        Self {
            num_actions,
            regrets: HashMap::new(),
            strategy_sum: HashMap::new(),
        }
    }

    pub fn add_regret(&mut self, infoset_hash: u64, action_idx: usize, delta: f32) {
        let entry = self
            .regrets
            .entry(infoset_hash)
            .or_insert_with(|| vec![0.0; self.num_actions]);
        entry[action_idx] += delta;
    }

    pub fn set_regret(&mut self, infoset_hash: u64, action_idx: usize, val: f32) {
        let entry = self
            .regrets
            .entry(infoset_hash)
            .or_insert_with(|| vec![0.0; self.num_actions]);
        entry[action_idx] = val;
    }

    pub fn add_strategy_sum(&mut self, infoset_hash: u64, action_idx: usize, prob: f32) {
        let entry = self
            .strategy_sum
            .entry(infoset_hash)
            .or_insert_with(|| vec![0.0; self.num_actions]);
        entry[action_idx] += prob;
    }

    pub fn get_strategy(&self, infoset_hash: u64) -> Vec<f32> {
        if let Some(r) = self.regrets.get(&infoset_hash) {
            let positive: Vec<f32> = r.iter().map(|&x| if x > 0.0 { x } else { 0.0 }).collect();
            let sum: f32 = positive.iter().sum();
            if sum > 0.0 {
                return positive.iter().map(|&p| p / sum).collect();
            }
        }
        vec![1.0 / self.num_actions as f32; self.num_actions]
    }

    pub fn get_average_strategy(&self, infoset_hash: u64) -> Vec<f32> {
        if let Some(s) = self.strategy_sum.get(&infoset_hash) {
            let sum: f32 = s.iter().sum();
            if sum > 0.0 {
                return s.iter().map(|&p| p / sum).collect();
            }
        }
        vec![1.0 / self.num_actions as f32; self.num_actions]
    }

    pub fn get_regret(&self, infoset_hash: u64, action_idx: usize) -> f32 {
        if let Some(r) = self.regrets.get(&infoset_hash) {
            return r[action_idx];
        }
        0.0
    }

    pub fn num_actions(&self) -> usize {
        self.num_actions
    }
    pub fn capacity(&self) -> usize {
        self.regrets.len()
    }
    pub fn get_keys(&self) -> Vec<u64> {
        self.strategy_sum.keys().copied().collect()
    }
}
