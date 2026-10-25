//! simplefp-core: the numerical engine. Pure functions over `&[f64]`, no I/O.

// Index loops read like the math they implement (Cholesky, normal equations).
#![allow(clippy::needless_range_loop)]

pub mod dsp;
pub mod fit;
pub mod stats;
pub mod events;
pub mod pipeline;
pub mod preset;
pub mod sim;
pub mod analysis;
pub mod group;
