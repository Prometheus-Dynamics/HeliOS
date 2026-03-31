use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogSourceKind {
    JournalSystem,
    JournalUnit,
    Dmesg,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema, Default)]
pub struct SystemdUnitStatus {
    pub active_state: Option<String>,
    pub sub_state: Option<String>,
    pub unit_file_state: Option<String>,
    pub description: Option<String>,
    pub fragment_path: Option<String>,
    pub main_pid: Option<u32>,
    pub exec_main_status: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema)]
pub struct LogSource {
    pub id: String,
    pub label: String,
    pub group: String,
    pub kind: LogSourceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub important: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<SystemdUnitStatus>,
}
