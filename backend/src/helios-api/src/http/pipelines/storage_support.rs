use axum::{Json, http::StatusCode, response::IntoResponse};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::fs;
use uuid::Uuid;

use crate::http::storage;

use super::{
    graph_support::{normalize_graph_metadata, unwrap_pipeline_export_graph},
    types::{PipelineDocument, PipelineError, PipelineTemplateDocument, PipelineTemplateDocumentRaw, PipelineTemplateSummary, map_io_error_response},
};

pub(crate) fn pipeline_dir() -> Result<PathBuf, Box<axum::response::Response>> {
    storage::ensure_subdir("pipelines").map_err(|err| Box::new(map_io_error(err, "failed to prepare pipeline directory")))
}

pub(crate) fn pipeline_template_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HELIOS_PIPELINE_TEMPLATE_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        let dev = cwd.join("configs").join("templates");
        if dev.is_dir() {
            return dev;
        }
    }
    PathBuf::from("/usr/share/helios/pipeline-templates")
}

pub(super) fn normalize_template_id(raw: &str) -> Option<String> {
    let sanitized = storage::sanitize_name(raw)?;
    let trimmed = sanitized.trim().trim_end_matches(".json");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

pub(super) async fn template_exists(raw_id: &str) -> bool {
    let Some(template_id) = normalize_template_id(raw_id) else {
        return false;
    };
    let path = pipeline_template_dir().join(format!("{template_id}.json"));
    match fs::metadata(path).await {
        Ok(meta) => meta.is_file(),
        Err(_) => false,
    }
}

pub(super) fn map_template_io_error<E: Into<std::io::Error>>(err: E, context: &str) -> axum::response::Response {
    let err = err.into();
    let status = if err.kind() == std::io::ErrorKind::NotFound { StatusCode::NOT_FOUND } else { StatusCode::INTERNAL_SERVER_ERROR };
    (status, Json(PipelineError { error: format!("{context}: {err}") })).into_response()
}

pub(crate) fn map_io_error<E: Into<std::io::Error>>(err: E, context: &str) -> axum::response::Response {
    map_io_error_response(err, context)
}

pub(crate) async fn load_graph_document_from_path(path: &Path) -> io::Result<PipelineDocument> {
    let data = fs::read(path).await?;
    PipelineDocument::decode_slice(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub async fn load_graph_document(id: Uuid) -> io::Result<PipelineDocument> {
    let dir = storage::ensure_subdir_async("pipelines").await?;
    let path = dir.join(format!("{id}.json"));
    load_graph_document_from_path(&path).await
}

fn template_name(template_id: &str, raw_name: Option<&str>) -> String {
    raw_name.map(str::trim).filter(|value| !value.is_empty()).unwrap_or(template_id).to_string()
}

fn template_summary(summary: Option<String>) -> Option<String> {
    summary.map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

pub(super) fn template_raw_into_summary(template_id: String, raw: PipelineTemplateDocumentRaw) -> PipelineTemplateSummary {
    let _raw_id = raw.id.as_deref().and_then(normalize_template_id);
    PipelineTemplateSummary { template_id: template_id.clone(), name: template_name(&template_id, raw.name.as_deref()), summary: template_summary(raw.summary), tags: raw.tags }
}

pub(super) fn template_raw_into_document(template_id: String, raw: PipelineTemplateDocumentRaw) -> PipelineTemplateDocument {
    let _raw_id = raw.id.as_deref().and_then(normalize_template_id);
    let mut graph = raw.graph;
    if let Some(unwrapped) = unwrap_pipeline_export_graph(&graph) {
        graph = unwrapped;
    }
    normalize_graph_metadata(&mut graph);
    PipelineTemplateDocument { id: template_id.clone(), name: template_name(&template_id, raw.name.as_deref()), summary: template_summary(raw.summary), tags: raw.tags, graph }
}

pub(crate) async fn load_template_graph(template_id: &str) -> io::Result<serde_json::Value> {
    let Some(template_id) = normalize_template_id(template_id) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid template id"));
    };
    let path = pipeline_template_dir().join(format!("{template_id}.json"));
    let data = fs::read_to_string(&path).await?;
    let raw = PipelineTemplateDocumentRaw::decode_str(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    Ok(template_raw_into_document(template_id, raw).graph)
}

#[cfg(test)]
mod tests {
    use super::{PipelineTemplateDocumentRaw, template_raw_into_summary};

    #[test]
    fn template_raw_into_summary_uses_template_id_when_name_is_blank() {
        let raw = PipelineTemplateDocumentRaw {
            schema_version: 1,
            id: Some("demo".to_string()),
            name: Some("   ".to_string()),
            summary: Some("  Example  ".to_string()),
            tags: vec!["vision".to_string()],
            graph: serde_json::json!({ "nodes": [], "edges": [] }),
        };

        let summary = template_raw_into_summary("demo".to_string(), raw);
        assert_eq!(summary.name, "demo");
        assert_eq!(summary.summary.as_deref(), Some("Example"));
    }
}
