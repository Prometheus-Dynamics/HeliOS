#![deny(unsafe_code)]

pub mod capture;
#[cfg(feature = "runtime")]
pub mod daedalus_registry;
pub mod error;
#[cfg(feature = "runtime")]
pub mod graph;
pub mod identity;
pub mod ipc;
pub mod localization;
#[cfg(feature = "runtime")]
pub mod pipelines;
#[cfg(feature = "runtime")]
pub mod runtime;
#[cfg(feature = "runtime")]
pub mod services;
pub mod stream;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
