#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here
pub mod session;
pub mod shadow;
pub mod subgame;
pub mod translate_live;
pub mod vsearch;

pub mod lookup;
pub mod mmap;

pub use lookup::{HealthReport, SolverHandle};
pub use mmap::{MmapError, MmapReader};
pub use session::RuntimeSession;
