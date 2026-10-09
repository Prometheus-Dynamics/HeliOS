#![deny(unsafe_code)]

pub mod config;
pub mod lemnosd;
pub mod model;
pub mod provider;
pub mod resources;
pub mod runtime;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
