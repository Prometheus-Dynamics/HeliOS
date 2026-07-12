#![deny(unsafe_code)]

pub mod boot_confirm;
pub mod client;
pub mod config;
pub mod coordinator;
pub mod engine;
pub mod executor;
pub mod manifest;
pub mod model;
pub mod plans;
pub mod provider;
pub mod releases;
pub mod runtime;
pub mod service;
pub mod slots;
pub mod staging;
pub mod update_core;
pub mod workloads;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
