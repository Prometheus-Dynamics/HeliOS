use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeviceSnapshotsResponse {
    pub snapshots: Vec<DeviceSnapshotResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeviceSnapshotResponse {
    pub id: String,
    pub label: String,
    pub created_by: String,
    pub created_at: String,
    pub size_bytes: u64,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CaptureSnapshotRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RequestedByQuery {
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct SnapshotMeta {
    pub(super) id: String,
    pub(super) created_utc: String,
    #[serde(default)]
    pub(super) host: Option<String>,
    #[serde(default)]
    pub(super) tag: Option<String>,
}
