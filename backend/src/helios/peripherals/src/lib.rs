#![deny(unsafe_code)]

pub mod config;
pub mod model;
pub mod provider;
pub mod resources;
pub mod runtime;
pub mod workloads;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
