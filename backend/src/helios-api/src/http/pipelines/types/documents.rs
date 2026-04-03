use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

pub(crate) const CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION: u32 = 1;
pub(crate) const CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION: u32 = 1;

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
        let migrated = normalize_to_current(raw, &PIPELINE_DOCUMENT_SCHEMA_PLAN)?;
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
pub(crate) struct PipelineTemplateDocumentRaw {
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
    pub(crate) fn decode_str(raw: &str) -> Result<Self, String> {
        let raw = serde_json::from_str::<serde_json::Value>(raw).map_err(|err| format!("failed to decode pipeline template document: {err}"))?;
        let migrated = normalize_to_current(raw, &PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_PLAN)?;
        let mut parsed: Self = serde_json::from_value(migrated).map_err(|err| format!("failed to parse pipeline template document: {err}"))?;
        parsed.schema_version = CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION;
        Ok(parsed)
    }
}

const PIPELINE_DOCUMENT_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("pipeline document", CURRENT_PIPELINE_DOCUMENT_SCHEMA_VERSION);

const PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("pipeline template document", CURRENT_PIPELINE_TEMPLATE_DOCUMENT_SCHEMA_VERSION);
