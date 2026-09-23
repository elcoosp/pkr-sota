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
        let keys = self.mmap.keys_data();
        let num_keys = keys.len() / 8;
        if num_keys == 0 {
            return None;
        }
        // Binary search the sorted key array
        let mut lo = 0;
        let mut hi = num_keys;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let k = u64::from_le_bytes(keys[mid * 8..mid * 8 + 8].try_into().unwrap());
            if k < infoset_hash {
                lo = mid + 1;
            } else if k > infoset_hash {
                hi = mid;
            } else {
                let max_actions = self.mmap.file_header().max_actions_k as usize;
                if max_actions > 16 {
                    return None;
                }
                let cdf_start = mid * max_actions;
                let cdf_end = cdf_start + max_actions;
                let cdf = self.mmap.cdf_data();
                if cdf_end > cdf.len() {
                    return None;
                }
                let mut prob = [0u8; 16];
                prob[..max_actions].copy_from_slice(&cdf[cdf_start..cdf_end]);
                return Some(SotaAdvice {
                    cdf_probabilities: prob,
                    len: max_actions as u8,
                });
            }
        }
        None
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
