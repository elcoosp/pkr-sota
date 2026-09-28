#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here
pub mod session;
pub mod subgame;
pub mod translate;

pub mod lookup;
pub mod mmap;

pub use lookup::SolverHandle;
pub use mmap::{MmapError, MmapReader};
