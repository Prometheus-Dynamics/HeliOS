//! Synthetic marker images for tests, benchmarks and the probe.
//!
//! Eidos has no public marker renderer yet, so HeliOS keeps the marker
//! codes and a renderer here; they are not used for detection.

mod dictionary;
pub mod render;

pub use dictionary::{DICT_4X4_50, Dictionary, TAG_36H11};
