// Dictionary data tables and OpenCV-predefined dictionaries.
//
// The `Dictionary` trait lives in the parent module; re-export it here so the
// per-dictionary data files can continue to `use super::Dictionary;`.

pub use super::Dictionary;

pub mod dict4x4_50;
pub mod dict5x5_100;
pub mod dict6x6_250;
pub mod dictionary_data;
