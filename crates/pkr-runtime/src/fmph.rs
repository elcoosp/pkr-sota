//! FMph (Finite State Machine Minimal Perfect Hash) — re-exported from pkr-export.
//! The runtime needs this for O(1) infoset lookup in the memory-mapped blueprint.
pub use pkr_export::fmph::{build_fmph, eval_fmph, FmphData};
