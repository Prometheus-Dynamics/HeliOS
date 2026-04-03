mod media;
mod options;
mod routes;
mod sidecar;
mod state;
mod types;

#[cfg(test)]
mod tests;

pub(crate) use routes::*;
pub(crate) use state::RecordingRuntimeState;
pub use types::{CaptureShadowRecordingRequest, StartRecordingRequest};
