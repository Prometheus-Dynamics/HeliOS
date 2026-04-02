use std::sync::Arc;

use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProcessSample {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub virtual_memory_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cmd: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SharedProcessesSnapshot {
    pub timestamp_ms: u64,
    pub total_memory_bytes: u64,
    pub used_memory_bytes: u64,
    pub processes: Arc<[ProcessSample]>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DevicesUpdateReason {
    Api,
    Pipelines,
    Localization,
    Media,
    Imu,
    Device,
    Settings,
    Usb,
    Streams,
}

#[derive(Debug, Clone)]
pub struct SharedDevicesUpdate {
    pub timestamp_ms: u64,
    pub reasons: Vec<DevicesUpdateReason>,
}
