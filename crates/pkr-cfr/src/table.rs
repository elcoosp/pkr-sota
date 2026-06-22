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

mod tests {
    use crate::CompactRegretTable;
    use std::sync::Arc;

    #[test]
    fn test_new_table_empty_strategy() {
        let table = CompactRegretTable::new();
        let mut out = [0.0f32; 6];
        table.get_strategy_into(42, &mut out);
        assert!((out.iter().sum::<f32>() - 1.0).abs() < 0.001);
        for v in out.iter() {
            assert!((*v - 1.0/6.0).abs() < 0.001);
        }
    }

    #[test]
    fn test_set_and_get_regret() {
        let table = CompactRegretTable::new();
        table.apply_regret_update(100, 2, 1, 5.0);
        let r = table.get_regret(100, 2);
        assert!(r > 0.0);
        assert!(r < 10.0);
    }

    #[test]
    fn test_strategy_from_positive_regrets() {
        let table = CompactRegretTable::new();
        // Set high regret for action 0, zero for others
        for _ in 0..10 {
            table.apply_regret_update(1, 0, 1, 10.0);
        }
        let mut out = [0.0f32; 6];
        table.get_strategy_into(1, &mut out);
        // Action 0 should have highest probability
        assert!(out[0] > out[1]);
        assert!(out[0] > 0.5);
    }

    #[test]
    fn test_strategy_sum_and_average() {
        let table = CompactRegretTable::new();
        // Accumulate strategy sum favoring action 3
        for _ in 0..100 {
            table.add_strategy_sum(7, 3, 0.8);
            table.add_strategy_sum(7, 0, 0.2);
        }
        let mut out = [0.0f32; 6];
        table.get_average_strategy_into(7, &mut out);
        assert!(out[3] > 0.7);
        assert!(out[0] < 0.3);
    }

    #[test]
    fn test_keys_collection() {
        let table = CompactRegretTable::new();
        table.add_strategy_sum(10, 0, 0.5);
        table.add_strategy_sum(20, 1, 0.5);
        table.add_strategy_sum(10, 2, 0.3);
        let keys = table.get_keys();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&10));
        assert!(keys.contains(&20));
    }

    #[test]
    fn test_get_average_strategy_slice() {
        let table = CompactRegretTable::new();
        table.add_strategy_sum(42, 1, 100.0);
        let slice = table.get_average_strategy_slice(42).unwrap();
        assert!(slice[1] > 0.9);
        assert!(table.get_average_strategy_slice(99).is_none());
    }

    #[test]
    fn test_multiple_regret_updates_converge() {
        let table = CompactRegretTable::new();
        // Regret for action 0 increases, others decrease
        for t in 1..=100 {
            table.apply_regret_update(5, 0, t, 1.0);
            table.apply_regret_update(5, 1, t, -0.5);
        }
        let r0 = table.get_regret(5, 0);
        let r1 = table.get_regret(5, 1);
        assert!(r0 > r1, "Action 0 regret should exceed action 1");
    }

    #[test]
    fn test_thread_safety() {
        use std::sync::Arc;
        use std::thread;
        let table = Arc::new(CompactRegretTable::new());
        let mut handles = vec![];
        for t in 0..4 {
            let t_clone = Arc::clone(&table);
            handles.push(thread::spawn(move || {
                for i in 0..100 {
                    t_clone.apply_regret_update(1, t, i, 1.0);
                    t_clone.add_strategy_sum(1, t, 0.25);
                }
            }));
        }
        for h in handles { h.join().unwrap(); }
        let mut out = [0.0f32; 6];
        table.get_strategy_into(1, &mut out);
        assert!((out.iter().sum::<f32>() - 1.0).abs() < 0.01);
    }
}
