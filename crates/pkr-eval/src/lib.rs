#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here

pub mod lookup;
pub mod slow;
pub use slow::NlheEvaluator;

#[cfg(feature = "fast-eval")]
pub mod lookup_fast;
#[cfg(feature = "fast-eval")]
pub use lookup_fast::TableEvaluator;

#[cfg(feature = "fast-eval")]
pub mod fast7;
#[cfg(feature = "fast-eval")]
pub use fast7::Fast7Evaluator;
