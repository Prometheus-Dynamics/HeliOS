//! HeliOS vision algorithms.
//!
//! CPU stages that work on a camera's luma plane, kept free of the graph
//! runtime so they can be tested and reused directly. The Daedalus nodes that
//! expose them live in the `plugin` module.

pub mod aruco;
pub mod contour;
pub mod geometry;
#[cfg(feature = "plugin")]
pub mod graphs;
pub mod image;
#[cfg(feature = "plugin")]
pub mod plugin;
pub mod quads;
pub mod refine;
pub mod threshold;

pub use aruco::{DecodeConfig, DetectorConfig, Dictionary, Marker, detect};
pub use image::{BinaryImage, GrayImage, ImageError};
pub use quads::{Quad, QuadConfig, find_quads};
pub use threshold::{ThresholdConfig, adaptive_threshold};
