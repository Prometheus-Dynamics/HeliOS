use axum::{Json, http::StatusCode, response::IntoResponse};
use std::{io, path::PathBuf};
use tokio::fs;
use uuid::Uuid;

use crate::http::storage;

use super::{
    graph_support::{normalize_graph_metadata, unwrap_pipeline_export_graph},
    types::{PipelineDocument, PipelineError, PipelineTemplateDocumentRaw, map_io_error_response},
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

pub async fn load_graph_document(id: Uuid) -> io::Result<PipelineDocument> {
    let dir = storage::ensure_subdir_async("pipelines").await?;
    let path = dir.join(format!("{id}.json"));
    let data = fs::read(&path).await?;
    serde_json::from_slice::<PipelineDocument>(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

pub(crate) async fn load_template_graph(template_id: &str) -> io::Result<serde_json::Value> {
    let Some(template_id) = normalize_template_id(template_id) else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid template id"));
    };
    let path = pipeline_template_dir().join(format!("{template_id}.json"));
    let data = fs::read_to_string(&path).await?;
    let raw = serde_json::from_str::<PipelineTemplateDocumentRaw>(&data).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let mut graph = raw.graph;
    if let Some(unwrapped) = unwrap_pipeline_export_graph(&graph) {
        graph = unwrapped;
    }
    normalize_graph_metadata(&mut graph);
    Ok(graph)
}
