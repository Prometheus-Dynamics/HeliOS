//! HeliOS vision: Daedalus nodes over Eidos for marker detection.
//!
//! The detection stages are Eidos operations wrapped as graph nodes
//! (`plugin`); `graphs` builds the ready-made graph documents. `refine` holds
//! the sub-pixel corner refinement until Eidos provides one, and `testing`
//! renders synthetic markers for tests.

pub mod geometry;
pub mod graphs;
pub mod image;
pub mod plugin;
pub mod refine;
pub mod testing;

pub use image::{GrayImage, GrayView, ImageError};
