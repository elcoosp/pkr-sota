pub mod lookup;
pub mod slow;
pub use slow::NlheEvaluator;

#[cfg(feature = "fast-eval")]
pub mod lookup_fast;
#[cfg(feature = "fast-eval")]
pub use lookup_fast::TableEvaluator;
