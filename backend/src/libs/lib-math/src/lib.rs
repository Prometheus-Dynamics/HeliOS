//! Core math utilities shared across Helios subsystems.
//!
//! The crate exposes runtime filter implementations and lightweight linear algebra
//! primitives so that downstream crates can configure signal processing and
//! geometry without bespoke glue.

pub mod filters;
pub mod linalg;

pub use filters::*;
pub use linalg::*;
