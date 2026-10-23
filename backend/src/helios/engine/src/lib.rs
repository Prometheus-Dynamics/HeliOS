pub mod config;
pub mod execution;
pub mod live;
pub mod model;
pub mod nt4;
pub mod plugins;
pub mod provider;
pub mod runtime;
pub mod stream_io;
pub mod workloads;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
