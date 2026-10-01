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
        //
        // F1 followup (bug hunt): the zero-sum case must match
        // `pkr_export::writer::quantize_cdf`. Before this fix it
        // emitted an all-zero prefix, which a consumer decodes as
        // "bucket 0 gets 100% of the mass" — not uniform. The exporter
        // emits a uniform CDF instead. Same encoding now.
        const K: usize = 6;
        let total: f32 = strat.iter().sum();
        let mut out = [0u8; 16];
        if total.is_nan() || total <= 0.0 {
            for a in 0..K {
                out[a] = ((((a + 1) as f32) / K as f32) * 255.0).round() as u8;
            }
            out[K - 1] = 255;
        } else {
            let mut cum = 0.0f32;
            let mut prev = 0u8;
            for a in 0..K {
                cum += strat[a] / total;
                let b = (cum * 255.0).round().clamp(0.0, 255.0) as u8;
                out[a] = b.max(prev);
                prev = out[a];
            }
            // Close on the last action that actually has probability.
            let last = (0..K).rev().find(|&a| strat[a] > 0.0).unwrap_or(K - 1);
            for a in last..K {
                out[a] = 255;
            }
        }
        for slot in out.iter_mut().take(16).skip(K) {
            *slot = 255;
        }
        Some(SotaAdvice {
            cdf_probabilities: out,
            len: K as u8,
        })
    }
}
