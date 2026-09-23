#![allow(clippy::needless_range_loop)] // numerics: indexed loops are idiomatic here

pub mod lookup;
pub mod mmap;

pub use lookup::SolverHandle;
pub use mmap::{MmapError, MmapReader};
