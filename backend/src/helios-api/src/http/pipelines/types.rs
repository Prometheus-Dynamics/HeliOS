use axum::response::IntoResponse;
use lib_schema_migration::{SyncSchemaPlan, migrate_to_current};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use utoipa::ToSchema;
use uuid::Uuid;

pub(crate) const CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION: u32 = 1;
const CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION: u32 = 1;

fn current_pipeline_document_schema_version() -> u32 {
    CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION
}

fn current_pipeline_template_document_schema_version() -> u32 {
    CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PipelineDocument {
    #[serde(default = "current_pipeline_document_schema_version")]
    pub schema_version: u32,
    pub id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    pub graph: serde_json::Value,
    pub updated_at_ms: i64,
}

impl PipelineDocument {
    pub fn new(id: Uuid, name: Option<String>, graph: serde_json::Value, updated_at_ms: i64) -> Self {
        Self { schema_version: CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION, id, name, graph, updated_at_ms }
    }

    pub fn ensure_supported_schema_version(&self) -> Result<(), String> {
        if self.schema_version > CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION {
            return Err(format!("unsupported pipeline document schema_version {}; current version is {}", self.schema_version, CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION));
        }
        Ok(())
    }

    pub fn decode_slice(bytes: &[u8]) -> Result<Self, String> {
        let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| format!("failed to decode pipeline document: {err}"))?;
        let migrated = migrate_to_current(raw, &PIPELINE_DOCUMENT_SCHEMA_PLAN)?;
        let mut parsed: Self = serde_json::from_value(migrated).map_err(|err| format!("failed to parse pipeline document: {err}"))?;
        parsed.schema_version = CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION;
        Ok(parsed)
    }

    pub fn encode_pretty(&self) -> Result<Vec<u8>, String> {
        let mut canonical = self.clone();
        canonical.schema_version = CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION;
        serde_json::to_vec_pretty(&canonical).map_err(|err| format!("failed to encode pipeline document: {err}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PipelineSummary {
    pub id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub issue_count: usize,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineTemplateSummary {
    pub template_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PipelineTemplateDocument {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub graph: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct PipelineTemplateDocumentRaw {
    #[serde(default = "current_pipeline_template_document_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub graph: serde_json::Value,
}

impl PipelineTemplateDocumentRaw {
    pub(super) fn decode_str(raw: &str) -> Result<Self, String> {
        let raw = serde_json::from_str::<serde_json::Value>(raw).map_err(|err| format!("failed to decode pipeline template document: {err}"))?;
        let migrated = migrate_to_current(raw, &PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_PLAN)?;
        let mut parsed: Self = serde_json::from_value(migrated).map_err(|err| format!("failed to parse pipeline template document: {err}"))?;
        parsed.schema_version = CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION;
        Ok(parsed)
    }
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

const PIPELINE_DOCUMENT_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> =
    SyncSchemaPlan { document_name: "pipeline document", legacy_version: CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION, current_version: CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION, migrations: &[] };

const PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan {
    document_name: "pipeline template document",
    legacy_version: CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION,
    current_version: CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION,
    migrations: &[],
};

#[cfg(test)]
mod tests {
    use super::{CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION, CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION, PipelineDocument, PipelineTemplateDocumentRaw};
    use uuid::Uuid;

    #[test]
    fn pipeline_document_decode_rejects_missing_schema_version() {
        let pipeline_id = Uuid::new_v4();
        let raw = serde_json::json!({
            "id": pipeline_id,
            "name": "demo",
            "graph": { "nodes": [], "edges": [] },
            "updated_at_ms": 123
        });

        let err = PipelineDocument::decode_slice(serde_json::to_string(&raw).expect("encode").as_bytes()).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn pipeline_document_decode_rejects_future_schema_version() {
        let raw = serde_json::json!({
            "schema_version": CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION + 1,
            "id": Uuid::new_v4(),
            "graph": { "nodes": [], "edges": [] },
            "updated_at_ms": 123
        });

        let err = PipelineDocument::decode_slice(serde_json::to_string(&raw).expect("encode").as_bytes()).expect_err("future version should fail");
        assert!(err.contains("unsupported pipeline document schema_version"));
    }

    #[test]
    fn pipeline_document_encode_stamps_current_schema_version() {
        let encoded =
            PipelineDocument { schema_version: 0, id: Uuid::new_v4(), name: None, graph: serde_json::json!({"nodes":[],"edges":[]}), updated_at_ms: 55 }.encode_pretty().expect("encode pipeline");
        let parsed: serde_json::Value = serde_json::from_slice(&encoded).expect("decode json");
        assert_eq!(parsed.get("schema_version").and_then(serde_json::Value::as_u64), Some(CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION as u64));
    }

    #[test]
    fn pipeline_template_document_decode_rejects_missing_schema_version() {
        let raw = serde_json::json!({
            "id": "demo",
            "name": "Demo Template",
            "graph": { "nodes": [], "edges": [] }
        });

        let err = PipelineTemplateDocumentRaw::decode_str(&serde_json::to_string(&raw).expect("encode")).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn pipeline_template_document_decode_rejects_future_schema_version() {
        let raw = serde_json::json!({
            "schema_version": CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION + 1,
            "id": "demo",
            "graph": { "nodes": [], "edges": [] }
        });

        let err = PipelineTemplateDocumentRaw::decode_str(&serde_json::to_string(&raw).expect("encode")).expect_err("future version should fail");
        assert!(err.contains("unsupported pipeline template document schema_version"));
    }
}
