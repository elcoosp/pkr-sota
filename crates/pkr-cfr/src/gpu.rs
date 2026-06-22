use std::collections::HashSet;
use std::sync::atomic::{AtomicI32, Ordering};
use dashmap::DashMap;
use crate::table::SCALE;

// Flat table for GPU-accelerated CFR updates
pub struct FlatRegretTable {
    pub regrets: Vec<AtomicI32>,       // length N*6, interleaved
    pub strategy_sum: Vec<AtomicI32>,
    pub hash_to_idx: DashMap<u64, usize>,
    pub idx_to_hash: Vec<u64>,
    pub num_infosets: usize,
}

impl FlatRegretTable {
    pub fn new() -> Self {
        Self {
            regrets: Vec::new(),
            strategy_sum: Vec::new(),
            hash_to_idx: DashMap::new(),
            idx_to_hash: Vec::new(),
            num_infosets: 0,
        }
    }

    /// Discover all infoset hashes from the given table and allocate flat arrays.
    pub fn build_from(&mut self, table: &crate::table::CompactRegretTable) {
        let keys: Vec<u64> = table.get_keys();
        self.num_infosets = keys.len();
        self.regrets = (0..self.num_infosets * 6).map(|_| AtomicI32::new(0)).collect();
        self.strategy_sum = (0..self.num_infosets * 6).map(|_| AtomicI32::new(0)).collect();

        for (idx, &hash) in keys.iter().enumerate() {
            self.hash_to_idx.insert(hash, idx);
            if idx >= self.idx_to_hash.len() {
                self.idx_to_hash.resize(idx + 1, 0);
            }
            self.idx_to_hash[idx] = hash;
        }
    }
}

// Placeholder for WGPU dispatch (if feature "gpu" is enabled)
#[cfg(feature = "gpu")]
pub fn dispatch_gpu_cfr_update(table: &FlatRegretTable, deltas: &[i32]) {
    // Implement wgpu setup and dispatch here.
    // On M1 with unified memory, this would be a near-zero-copy buffer pass.
    unimplemented!("GPU dispatch not yet implemented")
}
