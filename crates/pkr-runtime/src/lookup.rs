use crate::mmap::MmapReader;
use pkr_contracts::{BlueprintProvider, SotaAdvice};

pub struct SolverHandle {
    mmap: MmapReader,
}

impl SolverHandle {
    pub fn new(mmap: MmapReader) -> Self {
        SolverHandle { mmap }
    }

    /// P3-a: branchless binary search over a `&[u64]` cast from the mmap.
    /// One multiply-free comparison per probe, one load, no `from_le_bytes`.
    /// The key section is guaranteed 8-byte aligned because the writer
    /// emits it immediately after 4-byte header fields that total 8 mod 8.
    pub fn get_advice_fast(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        let keys: &[u64] = bytemuck::try_cast_slice(self.mmap.keys_data()).ok()?;
        let num_keys = keys.len();
        if num_keys == 0 {
            return None;
        }
        // Branchless upper-bound search: find the largest i with keys[i] <= target.
        let mut base = 0usize;
        let mut size = num_keys;
        while size > 1 {
            let half = size / 2;
            let mid = base + half;
            base = if keys[mid] <= infoset_hash { mid } else { base };
            size -= half;
        }
        if keys[base] != infoset_hash {
            return None;
        }
        self.advice_for_key_index(base)
    }

    /// Batch lookup for callers with many queries at once.
    ///
    /// Both `hashes` and the on-disk key table are sorted, so this walks
    /// them together in O(n log n + m) where n = `hashes.len()` and m =
    /// number of keys. That's one binary search *per call* avoided — for
    /// a large batch the per-hash cost drops from O(log m) probes to
    /// amortized O(1).
    ///
    /// The returned vec is parallel to the input: `result[i]` corresponds
    /// to `hashes[i]`, and equals `get_advice_fast(hashes[i])` byte for
    /// byte.
    pub fn get_advice_batch(&self, hashes: &[u64]) -> Vec<Option<SotaAdvice>> {
        let mut out: Vec<Option<SotaAdvice>> = vec![None; hashes.len()];
        if hashes.is_empty() {
            return out;
        }
        let keys: &[u64] = match bytemuck::try_cast_slice(self.mmap.keys_data()) {
            Ok(k) => k,
            Err(_) => return out,
        };
        let m = keys.len();
        if m == 0 {
            return out;
        }

        // Indices into `hashes`, sorted by hash value. Ties keep the
        // original order via the secondary index compare, so the output
        // is deterministic.
        let mut idx: Vec<usize> = (0..hashes.len()).collect();
        idx.sort_unstable_by_key(|&i| (hashes[i], i));

        let mut key_cursor = 0usize;
        for &i in &idx {
            let h = hashes[i];
            // Advance the key cursor to the first key >= h.
            while key_cursor < m && keys[key_cursor] < h {
                key_cursor += 1;
            }
            if key_cursor < m && keys[key_cursor] == h {
                out[i] = self.advice_for_key_index(key_cursor);
            }
        }
        out
    }

    /// Shared tail of single and batch lookup: given a known-valid index
    /// into the key table, produce the `SotaAdvice`. Returns `None` if
    /// the blueprint's header or CDF section is malformed.
    fn advice_for_key_index(&self, key_index: usize) -> Option<SotaAdvice> {
        let max_actions = self.mmap.file_header().max_actions_k as usize;
        if max_actions == 0 || max_actions > 16 {
            return None;
        }
        let cdf = self.mmap.cdf_data();
        let cdf_start = key_index * max_actions;
        let cdf_end = cdf_start + max_actions;
        if cdf_end > cdf.len() {
            return None;
        }
        let mut prob = [0u8; 16];
        prob[..max_actions].copy_from_slice(&cdf[cdf_start..cdf_end]);
        Some(SotaAdvice {
            cdf_probabilities: prob,
            len: max_actions as u8,
        })
    }
}

impl SolverHandle {
    /// T2.1: translated lookup. The host app has a desired bet amount
    /// (chips) that may not exactly match any trained anchor. This method
    /// fetches the trained CDF and redistributes the bet-bucket mass
    /// between the two bracketing anchors via pseudo-harmonic translation
    /// (Ganzfried & Sandholm 2013).
    ///
    /// Returns None if the infoset is not in the blueprint; the host app
    /// should then fall back to `fallback_advice`.
    ///
    /// `street` is the street code (0=preflop, 1=flop, 2=turn, 3=river).
    /// `requested_amount` is the bet/raise size in chips (not pot fraction).
    /// `pot` is the current pot size in chips.
    pub fn get_advice_translated(
        &self,
        infoset_hash: u64,
        street: u8,
        pot: f32,
        requested_amount: f32,
    ) -> Option<SotaAdvice> {
        let advice = self.get_advice_fast(infoset_hash)?;
        let street_idx = (street as usize).min(3);
        let anchors = self.mmap.anchors()[street_idx];
        Some(crate::translate::resolve_action(
            &advice,
            &anchors,
            requested_amount,
            pot,
        ))
    }

    /// Conservative fallback advice for lookup misses.
    ///
    /// The host application should call this when `get_advice_fast`
    /// returns `None`. That happens for infosets the trainer never
    /// visited, or off-abstraction opponent actions.
    ///
    /// The distribution is deliberately mild: ~10% fold, ~40%
    /// check/call, then progressively less mass on bigger bets. It
    /// loses nothing versus uniform random and avoids the pathological
    /// "always fold" and "always jam" ends. Host apps that want a
    /// smarter fallback (pot-odds, preflop chart) should layer that on
    /// top of this.
    pub fn fallback_advice(&self) -> SotaAdvice {
        SotaAdvice {
            cdf_probabilities: [
                26, 128, 179, 204, 230, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ],
            len: 6,
        }
    }
}

impl BlueprintProvider for SolverHandle {
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        self.get_advice_fast(infoset_hash)
    }
}

impl SolverHandle {
    /// Raw bytes of the sorted key table. Intended for diagnostics and tests;
    /// do not use on the hot path.
    pub fn debug_keys(&self) -> &[u8] {
        self.mmap.keys_data()
    }
}
