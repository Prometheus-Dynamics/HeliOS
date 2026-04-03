use std::{env, path::PathBuf};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Utc;
use helios_updater::ipc::{MaintenanceWindow, PreflightReport, UpdateStage, UpdateState, UpdaterCommand, UpdaterStorageReport};
use helios_updater::{ManifestArtifact, ReleaseManifest, ReleaseManifestMetadata};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::time::{Duration, Instant, sleep};
use tracing::warn;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AppState;
use super::ota_storage;
use super::storage::sanitize_name;
use super::upload_integrity;
use crate::ipc::command_id_from_context;

pub(crate) mod state;
#[cfg(test)]
mod tests;
mod types;
pub(crate) mod upload;
pub(crate) mod workflow;

use state::updater_state;
use upload::upload_update;
use workflow::{apply_update, cancel_update, preflight_update, stage_update};

pub use types::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/upload", post(upload_update))
        .route("/stage", post(stage_update))
        .route("/preflight", post(preflight_update))
        .route("/apply", post(apply_update))
        .route("/cancel", post(cancel_update))
        .route("/state", get(updater_state))
        .route_layer(DefaultBodyLimit::disable())
}
