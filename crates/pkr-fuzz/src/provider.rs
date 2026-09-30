//! `BlueprintProvider` adapters for real checkpoints.
//!
//! `SolverHandle` in `pkr-runtime` serves a *quantized blueprint* —
//! the exported u8 CDFs, mmap'd. That's the shipping path.
//!
//! Tests and evaluation harnesses need to serve a *live checkpoint*:
//! the `CompactRegretTable` that the trainer holds mid-run, before
//! export. `TableProvider` fills that gap.
//!
//! The CDF encoding matches `pkr-export::writer::quantize_cdf` so a
//! checkpoint evaluated through `TableProvider` and the same
//! checkpoint exported then read through `SolverHandle` should give
//! the same decision distribution (modulo u8 rounding).

use pkr_cfr::table::CompactRegretTable;
use pkr_contracts::{BlueprintProvider, SotaAdvice};

/// Wrap a `CompactRegretTable` as a `BlueprintProvider`.
pub struct TableProvider<'a> {
    table: &'a CompactRegretTable,
}

impl<'a> TableProvider<'a> {
    pub fn new(table: &'a CompactRegretTable) -> Self {
        TableProvider { table }
    }
}

impl<'a> BlueprintProvider for TableProvider<'a> {
    fn lookup(&self, infoset_hash: u64) -> Option<SotaAdvice> {
        let strat = self.table.get_average_strategy_slice(infoset_hash)?;
        // Encode as a cumulative CDF over the 6 buckets, then pad to
        // 16 slots with 255 (the exporter's convention).
        let total: f32 = strat.iter().sum();
        let mut out = [0u8; 16];
        let mut cum = 0.0f32;
        let mut prev = 0u8;
        for i in 0..6 {
            cum += if total > 1e-9 { strat[i] / total } else { 0.0 };
            let b = (cum * 255.0).round().clamp(0.0, 255.0) as u8;
            out[i] = b.max(prev);
            prev = out[i];
        }
        for slot in out.iter_mut().take(16).skip(6) {
            *slot = 255;
        }
        Some(SotaAdvice {
            cdf_probabilities: out,
            len: 6,
        })
    }
}
