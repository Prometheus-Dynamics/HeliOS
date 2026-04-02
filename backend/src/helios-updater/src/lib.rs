#![deny(unsafe_code)]

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(feature = "updater-ipc")]
mod apply;
#[cfg(any(feature = "updater-ipc", feature = "dto"))]
mod artifact;
#[cfg(feature = "updater-ipc")]
mod bundle;
#[cfg(feature = "updater-ipc")]
mod cleanup;
#[cfg(feature = "updater-ipc")]
pub mod client;
#[cfg(feature = "updater-ipc")]
mod config;
#[cfg(feature = "updater-ipc")]
mod error;
pub mod ipc;
#[cfg(feature = "updater-ipc")]
mod runtime;
#[cfg(feature = "updater-ipc")]
mod service;
#[cfg(feature = "updater-ipc")]
mod state;
#[cfg(feature = "updater-ipc")]
pub mod update_core;
#[cfg(feature = "updater-ipc")]
mod util;

#[cfg(any(feature = "updater-ipc", feature = "dto"))]
pub use artifact::{ManifestArtifact, ReleaseManifest, ReleaseManifestMetadata, StagedArtifact, StagedMetadata};
#[cfg(feature = "updater-ipc")]
pub use config::{SignaturePolicy, UpdaterConfig};
#[cfg(feature = "updater-ipc")]
pub use error::{Error, Result};
#[cfg(feature = "updater-ipc")]
pub use runtime::UpdaterRuntime;

#[cfg(feature = "updater-ipc")]
pub use client::{CommandId as UpdaterCommandId, Error as ClientError, Result as ClientResult, UpdaterClient, UpdaterClientConfig, UpdaterSession, types as ipc_types};

#[cfg(all(feature = "updater-ipc", feature = "mock"))]
pub use client::MockUpdater;

#[cfg(all(feature = "updater-ipc", test))]
mod tests;
