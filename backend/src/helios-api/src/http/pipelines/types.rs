use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use utoipa::ToSchema;
use uuid::Uuid;

mod documents;
#[cfg(test)]
mod tests;

pub use documents::PipelineDocument;
pub(crate) use documents::PipelineTemplateDocumentRaw;
pub use documents::{PipelineTemplateDocument, PipelineTemplateSummary};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PipelineSummary {
    pub id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub issue_count: usize,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UploadGraphRequest {
    #[serde(default)]
    pub name: Option<String>,
    pub graph: serde_json::Value,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PipelineError {
    pub error: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DaedalusRegistryPort {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ty: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub const_value: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DaedalusRegistryFanInPort {
    pub prefix: String,
    #[serde(default)]
    pub start: u32,
    pub ty: serde_json::Value,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DaedalusRegistryNode {
    pub id: String,
    pub label: Option<String>,
    pub plugin: Option<String>,
    pub feature_flags: Vec<String>,
    pub sync_groups: Vec<DaedalusSyncGroup>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_ports: Vec<DaedalusRegistryPort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fanin_inputs: Vec<DaedalusRegistryFanInPort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_ports: Vec<DaedalusRegistryPort>,
    pub default_compute: String,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DaedalusSyncGroup {
    pub name: String,
    pub policy: String,
    pub ports: Vec<String>,
    pub capacity: Option<usize>,
    pub backpressure: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DaedalusRegistryResponse {
    pub plugins: Vec<String>,
    pub nodes: Vec<DaedalusRegistryNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<DaedalusRegistryType>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DaedalusRegistryType {
    pub rust: String,
    pub ty: serde_json::Value,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ValidateGraphRequest {
    #[serde(default)]
    pub graph_id: Option<Uuid>,
    pub graph: serde_json::Value,
    #[serde(default)]
    pub active_features: Vec<String>,
    #[serde(default)]
    pub enable_lints: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlannerDiagnosticSpan {
    pub pass: String,
    pub node: Option<String>,
    pub port: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlannerDiagnostic {
    pub code: String,
    pub message: String,
    pub span: PlannerDiagnosticSpan,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ValidateGraphResponse {
    pub ok: bool,
    pub diagnostics: Vec<PlannerDiagnostic>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub gpu_segments: Vec<GpuSegment>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub gpu_edges: Vec<GpuEdgeBufferInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub node_ids: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GpuSegment {
    pub buffer_id: usize,
    pub nodes: Vec<usize>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GpuEdgeBufferInfo {
    pub edge_index: usize,
    pub gpu_fast_path: bool,
    pub buffer_id: Option<usize>,
}

pub(crate) type RegistryPortMetadataLookup = HashMap<String, BTreeMap<String, serde_json::Value>>;

#[derive(Debug)]
pub(crate) struct PipelineRefreshFailure {
    pub stream_id: Uuid,
    pub error: String,
}

pub(crate) fn map_io_error_response<E: Into<std::io::Error>>(err: E, context: &str) -> axum::response::Response {
    let err = err.into();
    let status = if err.kind() == std::io::ErrorKind::NotFound { axum::http::StatusCode::NOT_FOUND } else { axum::http::StatusCode::INTERNAL_SERVER_ERROR };
    (status, axum::Json(PipelineError { error: format!("{context}: {err}") })).into_response()
}
