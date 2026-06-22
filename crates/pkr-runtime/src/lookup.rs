// lookup.rs
use crate::mmap::MmapReader;
use pkr_contracts::{BlueprintProvider, SotaAdvice};

pub struct SolverHandle {
    mmap: MmapReader,
}

impl SolverHandle {
    pub fn new(mmap: MmapReader) -> Self {
        SolverHandle { mmap }
    }

    pub fn get_advice_fast(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        let fh = self.mmap.file_header();
        if fh.infoset_count == 0 {
            return None;
        }

        let num_keys = self.mmap.fmph_header().num_keys as usize;
        let idx = eval_mph(
            infoset_hash,
            num_keys,
            self.mmap.fmph_header(),
            self.mmap.fmph_data(),
        );

        let max_actions = fh.max_actions_k as usize;
        let cdf_start = idx * max_actions;
        let cdf_end = cdf_start + max_actions;
        let cdf_slice = self.mmap.cdf_data();

        if cdf_end > cdf_slice.len() {
            return None;
        }

        Some(SotaAdvice {
            cdf_probabilities: cdf_slice[cdf_start..cdf_end].to_vec(),
        })
    }
}

impl BlueprintProvider for SolverHandle {
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        self.get_advice_fast(infoset_hash)
    }
}

#[inline]
fn hash_key(key: u64, seed: u64) -> u64 {
    key.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(seed)
}

fn eval_mph(
    key: u64,
    num_keys: usize,
    fmph_header: &pkr_export::header::FmphHeader,
    fmph_data: &[u8],
) -> usize {
    let max_level_size = fmph_header.max_level_size as usize;
    let seed1 = fmph_header.seed1;
    let seed2 = fmph_header.seed2;

    let bucket = hash_key(key, seed1) as usize % max_level_size;
    let d = u32::from_le_bytes(fmph_data[bucket * 4..bucket * 4 + 4].try_into().unwrap()) as u64;

    (hash_key(key, seed2).wrapping_add(d) as usize) % num_keys
}

// mmap.rs remains structurally the same, just ensure FmphHeader size assertions are 40 bytes now.
