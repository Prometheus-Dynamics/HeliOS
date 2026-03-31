mod naming;
mod publish;
mod runtime;
mod snapshot;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde::Serialize;
use utoipa::ToSchema;

use super::limelight_types::{LimelightControlState, LimelightReadSnapshot};
use crate::ipc::IpcHandles;

pub fn init(handles: Arc<IpcHandles>) {
    runtime::init(handles);
}

pub async fn registry_status() -> LimelightAdapterRegistryStatus {
    runtime::registry_status().await
}

pub async fn adapter_status(table: &str) -> Option<LimelightAdapterStatus> {
    runtime::adapter_status(table).await
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct LimelightAdapterStatus {
    pub table_name: String,
    pub stream_id: uuid::Uuid,
    #[serde(default)]
    pub stream_alias: Option<String>,
    pub stream_state: String,
    pub recording_active: bool,
    pub publish_phase: String,
    pub publish_ready: bool,
    pub cached_topic_count: usize,
    pub updated_at_ms: u64,
    pub read_snapshot: LimelightReadSnapshot,
    pub control_state: LimelightControlState,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct LimelightAdapterRegistryStatus {
    pub emulate_enabled: bool,
    pub publish_phase: String,
    pub publish_ready: bool,
    pub adapter_count: usize,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub adapters: Vec<LimelightAdapterStatus>,
}
