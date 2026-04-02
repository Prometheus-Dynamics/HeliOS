mod graph_support;
mod identity;
mod refresh;
mod registry;
mod storage_support;
mod types;

pub(crate) use graph_support::{
    build_registry_port_metadata_lookup, inject_pipeline_alias_metadata, inject_port_metadata, inject_port_metadata_lookup, merge_edge_metadata, normalize_graph_metadata, normalize_graph_node_ids,
};
pub(crate) use refresh::{refresh_pipeline_consumers, refresh_pipeline_input_consumers};
pub(crate) use registry::refresh_graph_validation;
pub use registry::warm_registry_cache;
pub(crate) use storage_support::{load_graph_document, load_template_graph, map_io_error, pipeline_dir, pipeline_template_dir};
pub use types::{
    DaedalusRegistryFanInPort, DaedalusRegistryNode, DaedalusRegistryPort, DaedalusRegistryResponse, DaedalusRegistryType, DaedalusSyncGroup, GpuEdgeBufferInfo, GpuSegment, PipelineDocument,
    PipelineError, PipelineSummary, PipelineTemplateDocument, PipelineTemplateSummary, PlannerDiagnostic, PlannerDiagnosticSpan, UploadGraphRequest, ValidateGraphRequest, ValidateGraphResponse,
};
pub(crate) use types::{PipelineRefreshFailure, RegistryPortMetadataLookup};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::IntoResponse,
    routing::{get, post},
};
use std::{io, sync::OnceLock};
use tokio::fs;
use uuid::Uuid;

use self::{
    graph_support::{map_planner_diagnostics, pipeline_graph_alias, unwrap_pipeline_export_graph},
    identity::{ensure_unique_pipeline_identity, pipeline_identity_conflict},
    refresh::detach_pipeline_from_streams,
    registry::{clear_graph_validation_state, inject_cached_port_metadata, invalidate_graph_list_cache, set_graph_validation_error, set_graph_validation_state},
    storage_support::{load_graph_document_from_path, map_template_io_error, normalize_template_id, template_exists, template_raw_into_document, template_raw_into_summary},
    types::PipelineTemplateDocumentRaw,
};
use super::{
    AppState,
    revision::{apply_revision_headers, matches_if_none_match, not_modified_response},
};
use crate::http::streams::types::EngineErrorBody;
use crate::http::streams::util::{engine_error_body, map_client_error};
use crate::pipelines_read_model::{GraphValidationRequestError, validate_graph_report};
use helios_engine::ipc::EngineErrorCode;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/graphs", post(upload_graph).get(list_graphs))
        .route("/graphs/{id}", get(fetch_graph).put(update_graph).delete(delete_graph))
        .route("/templates", get(list_templates))
        .route("/templates/{id}", get(fetch_template))
        .route("/registry", get(list_registry))
        .route("/validate", post(validate_graph))
}

fn pipeline_graph_write_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

#[utoipa::path(
    get,
    path = "/pipelines/registry",
    tag = "Pipelines",
    responses((status = 200, description = "Daedalus node registry", body = DaedalusRegistryResponse))
)]
async fn list_registry(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let (payload, stale, revision) = match state.services.pipelines.get_cached_registry_response_snapshot(&state).await {
        Some(value) => value,
        None => {
            return (StatusCode::BAD_GATEWAY, Json(PipelineError { error: "registry unavailable".to_string() })).into_response();
        }
    };

    if matches_if_none_match(&headers, revision) {
        return not_modified_response(revision);
    }

    let mut response = axum::response::Response::new(Body::from(payload));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    apply_revision_headers(response.headers_mut(), revision);
    if stale {
        response.headers_mut().insert("x-helios-registry-stale", HeaderValue::from_static("1"));
    }
    response
}

#[utoipa::path(
    post,
    path = "/pipelines/validate",
    tag = "Pipelines",
    request_body = ValidateGraphRequest,
    responses(
        (status = 200, description = "Planner diagnostics", body = ValidateGraphResponse),
        (status = 400, description = "Engine validation rejected the graph", body = EngineErrorBody),
        (status = 502, description = "Planner error", body = EngineErrorBody)
    )
)]
async fn validate_graph(State(state): State<AppState>, Json(payload): Json<ValidateGraphRequest>) -> impl IntoResponse {
    let report = match validate_graph_report(&state, payload.graph, payload.active_features, payload.enable_lints).await {
        Ok(report) => report,
        Err(GraphValidationRequestError::Rejected { code, reason }) => {
            if let Some(graph_id) = payload.graph_id {
                set_graph_validation_error(&state, graph_id, Some(code), reason.clone()).await;
            }
            return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(code), reason))).into_response();
        }
        Err(GraphValidationRequestError::Transport(err)) => {
            if let Some(graph_id) = payload.graph_id {
                set_graph_validation_error(&state, graph_id, Some(EngineErrorCode::Internal), format!("validation failed: {err}")).await;
            }
            return map_client_error(err);
        }
    };

    let helios_engine::ipc::GraphValidationReport { ok, diagnostics: raw_diagnostics, gpu_segments, gpu_edges, node_ids } = report;
    let diagnostics = map_planner_diagnostics(raw_diagnostics);
    if let Some(graph_id) = payload.graph_id {
        set_graph_validation_state(&state, graph_id, diagnostics.clone()).await;
    }

    Json(ValidateGraphResponse {
        ok,
        diagnostics,
        gpu_segments: gpu_segments.into_iter().map(|segment| GpuSegment { buffer_id: segment.buffer_id, nodes: segment.nodes }).collect(),
        gpu_edges: gpu_edges.into_iter().map(|edge| GpuEdgeBufferInfo { edge_index: edge.edge_index, gpu_fast_path: edge.gpu_fast_path, buffer_id: edge.buffer_id }).collect(),
        node_ids,
    })
    .into_response()
}

#[utoipa::path(
    post,
    path = "/pipelines/graphs",
    tag = "Pipelines",
    request_body = UploadGraphRequest,
    responses(
        (status = 201, description = "Graph stored", body = PipelineDocument),
        (status = 400, description = "Invalid payload", body = PipelineError),
        (status = 500, description = "Storage error", body = PipelineError)
    )
)]
async fn upload_graph(State(state): State<AppState>, Json(payload): Json<UploadGraphRequest>) -> impl IntoResponse {
    let dir = match pipeline_dir() {
        Ok(dir) => dir,
        Err(resp) => return *resp,
    };
    let explicit_name = payload.name.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(|v| v.to_string());
    let mut name = explicit_name.clone();
    let explicit_name_provided = explicit_name.is_some();

    let mut raw_graph = payload.graph.clone();
    if let Some(unwrapped) = unwrap_pipeline_export_graph(&raw_graph) {
        raw_graph = unwrapped;
    }
    normalize_graph_metadata(&mut raw_graph);

    let mut graph: daedalus::planner::Graph = match serde_json::from_value(raw_graph.clone()) {
        Ok(graph) => graph,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(PipelineError { error: format!("invalid daedalus graph: {err}") })).into_response();
        }
    };
    normalize_graph_node_ids(&mut graph);
    let mut graph_json = match serde_json::to_value(&graph) {
        Ok(value) => value,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(PipelineError { error: format!("invalid daedalus graph: {err}") })).into_response();
        }
    };
    inject_cached_port_metadata(&state, &mut graph_json).await;
    merge_edge_metadata(&payload.graph, &mut graph_json);

    let _guard = pipeline_graph_write_lock().lock().await;
    let id = Uuid::new_v4();

    if !explicit_name_provided && let Some(alias) = pipeline_graph_alias(&graph_json) {
        name = Some(alias.to_string());
    }

    inject_pipeline_alias_metadata(&mut graph_json, name.as_deref());

    if !explicit_name_provided {
        const MAX_ATTEMPTS: usize = 100;
        let base = name.clone().unwrap_or_default();
        let base = base.trim().to_string();
        if !base.is_empty() {
            let mut selected = None;
            for attempt in 0..MAX_ATTEMPTS {
                let candidate = if attempt == 0 { base.clone() } else { format!("{base}-{}", attempt + 1) };
                inject_pipeline_alias_metadata(&mut graph_json, Some(&candidate));
                match pipeline_identity_conflict(&dir, id, Some(&candidate), &graph_json).await {
                    Ok(None) => {
                        selected = Some(candidate);
                        break;
                    }
                    Ok(Some(_)) => continue,
                    Err(resp) => return resp,
                }
            }
            if let Some(chosen) = selected {
                name = Some(chosen);
            } else {
                return (StatusCode::CONFLICT, Json(PipelineError { error: "failed to choose a unique pipeline name".into() })).into_response();
            }
        }
    }

    if let Some(resp) = ensure_unique_pipeline_identity(&dir, id, name.as_deref(), &graph_json).await {
        return resp;
    }
    let doc = PipelineDocument::new(id, name, graph_json, chrono::Utc::now().timestamp_millis());
    let path = dir.join(format!("{id}.json"));
    let data = match doc.encode_pretty() {
        Ok(bytes) => bytes,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(PipelineError { error: format!("invalid graph payload: {err}") })).into_response();
        }
    };
    match fs::write(path, data).await {
        Ok(_) => {
            invalidate_graph_list_cache(&state).await;
            refresh_graph_validation(&state, id, &doc.graph).await;
            (StatusCode::CREATED, Json(doc)).into_response()
        }
        Err(err) => map_io_error(err, "failed to store graph"),
    }
}

#[utoipa::path(
    put,
    path = "/pipelines/graphs/{id}",
    tag = "Pipelines",
    params(("id" = Uuid, Path, description = "Graph identifier")),
    request_body = UploadGraphRequest,
    responses(
        (status = 200, description = "Graph updated", body = PipelineDocument),
        (status = 400, description = "Invalid payload", body = PipelineError),
        (status = 404, description = "Graph not found", body = PipelineError),
        (status = 500, description = "Storage error", body = PipelineError)
    )
)]
async fn update_graph(State(state): State<AppState>, Path(id): Path<Uuid>, Json(payload): Json<UploadGraphRequest>) -> impl IntoResponse {
    if template_exists(id.to_string().as_str()).await {
        return (StatusCode::FORBIDDEN, Json(PipelineError { error: "template graphs are read-only".into() })).into_response();
    }
    let dir = match pipeline_dir() {
        Ok(dir) => dir,
        Err(resp) => return *resp,
    };
    let path = dir.join(format!("{id}.json"));
    let explicit_name = payload.name.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(|v| v.to_string());
    let mut name = explicit_name.clone();
    let explicit_name_provided = explicit_name.is_some();

    match fs::metadata(&path).await {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => {
            return (StatusCode::NOT_FOUND, Json(PipelineError { error: "graph not found".into() })).into_response();
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (StatusCode::NOT_FOUND, Json(PipelineError { error: "graph not found".into() })).into_response();
        }
        Err(err) => return map_io_error(err, "failed to stat graph"),
    }

    let existing_doc = match fs::read(&path).await {
        Ok(data) => PipelineDocument::decode_slice(&data).ok(),
        Err(_) => None,
    };

    let mut raw_graph = payload.graph.clone();
    if let Some(unwrapped) = unwrap_pipeline_export_graph(&raw_graph) {
        raw_graph = unwrapped;
    }
    normalize_graph_metadata(&mut raw_graph);

    let mut graph: daedalus::planner::Graph = match serde_json::from_value(raw_graph.clone()) {
        Ok(graph) => graph,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(PipelineError { error: format!("invalid daedalus graph: {err}") })).into_response();
        }
    };
    normalize_graph_node_ids(&mut graph);
    let mut graph_json = match serde_json::to_value(&graph) {
        Ok(value) => value,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(PipelineError { error: format!("invalid daedalus graph: {err}") })).into_response();
        }
    };
    inject_cached_port_metadata(&state, &mut graph_json).await;
    merge_edge_metadata(&payload.graph, &mut graph_json);

    let _guard = pipeline_graph_write_lock().lock().await;

    if !explicit_name_provided {
        if let Some(alias) = pipeline_graph_alias(&graph_json) {
            name = Some(alias.to_string());
        } else if let Some(existing) = existing_doc.as_ref().and_then(|doc| doc.name.clone()) {
            name = Some(existing);
        }
    }

    inject_pipeline_alias_metadata(&mut graph_json, name.as_deref());
    if let Some(resp) = ensure_unique_pipeline_identity(&dir, id, name.as_deref(), &graph_json).await {
        return resp;
    }
    let doc = PipelineDocument::new(id, name, graph_json, chrono::Utc::now().timestamp_millis());
    let data = match doc.encode_pretty() {
        Ok(bytes) => bytes,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(PipelineError { error: format!("invalid graph payload: {err}") })).into_response();
        }
    };
    match fs::write(path, data).await {
        Ok(_) => {
            invalidate_graph_list_cache(&state).await;
            refresh_graph_validation(&state, id, &doc.graph).await;
            let failures = refresh_pipeline_consumers(&state, id, &doc.graph).await;
            if !failures.is_empty() {
                let detail = failures.iter().map(|failure| format!("{}: {}", failure.stream_id, failure.error)).collect::<Vec<_>>().join(", ");
                return (StatusCode::CONFLICT, Json(PipelineError { error: format!("graph saved but failed to refresh streams: {detail}") })).into_response();
            }
            (StatusCode::OK, Json(doc)).into_response()
        }
        Err(err) => map_io_error(err, "failed to store graph"),
    }
}

#[utoipa::path(
    get,
    path = "/pipelines/graphs",
    tag = "Pipelines",
    responses((status = 200, description = "List stored graphs", body = [PipelineSummary]), (status = 500, description = "Storage error", body = PipelineError))
)]
async fn list_graphs(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match state.services.pipelines.get_cached_graph_summaries_snapshot(&state).await {
        Ok((summaries, revision)) => {
            if matches_if_none_match(&headers, revision) {
                return not_modified_response(revision);
            }
            let mut response = Json(summaries.as_ref().clone()).into_response();
            apply_revision_headers(response.headers_mut(), revision);
            response
        }
        Err(resp) => *resp,
    }
}

#[utoipa::path(
    get,
    path = "/pipelines/graphs/{id}",
    tag = "Pipelines",
    params(("id" = Uuid, Path, description = "Graph identifier")),
    responses((status = 200, description = "Graph document", body = PipelineDocument), (status = 404, description = "Graph not found", body = PipelineError))
)]
async fn fetch_graph(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    let dir = match pipeline_dir() {
        Ok(dir) => dir,
        Err(resp) => return *resp,
    };
    let path = dir.join(format!("{id}.json"));
    match load_graph_document_from_path(&path).await {
        Ok(mut doc) => {
            inject_cached_port_metadata(&state, &mut doc.graph).await;
            Json(doc).into_response()
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (StatusCode::NOT_FOUND, Json(PipelineError { error: "graph not found".into() })).into_response(),
        Err(err) if err.kind() == io::ErrorKind::InvalidData => (StatusCode::INTERNAL_SERVER_ERROR, Json(PipelineError { error: format!("failed to decode graph: {err}") })).into_response(),
        Err(err) => map_io_error(err, "failed to read graph"),
    }
}

#[utoipa::path(
    delete,
    path = "/pipelines/graphs/{id}",
    tag = "Pipelines",
    params(("id" = Uuid, Path, description = "Graph identifier")),
    responses(
        (status = 204, description = "Graph deleted"),
        (status = 404, description = "Graph not found", body = PipelineError),
        (status = 500, description = "Storage error", body = PipelineError)
    )
)]
async fn delete_graph(State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    if template_exists(id.to_string().as_str()).await {
        return (StatusCode::FORBIDDEN, Json(PipelineError { error: "template graphs are read-only".into() })).into_response();
    }
    let dir = match pipeline_dir() {
        Ok(dir) => dir,
        Err(resp) => return *resp,
    };
    let path = dir.join(format!("{id}.json"));
    match fs::remove_file(&path).await {
        Ok(()) => {
            invalidate_graph_list_cache(&state).await;
            clear_graph_validation_state(&state, id).await;
            match detach_pipeline_from_streams(&state, id).await {
                Ok(()) => StatusCode::NO_CONTENT.into_response(),
                Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, Json(PipelineError { error: err })).into_response(),
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (StatusCode::NOT_FOUND, Json(PipelineError { error: "graph not found".into() })).into_response(),
        Err(err) => map_io_error(err, "failed to delete graph"),
    }
}

#[utoipa::path(
    get,
    path = "/pipelines/templates",
    tag = "Pipelines",
    responses((status = 200, description = "List template graphs", body = [PipelineTemplateSummary]))
)]
async fn list_templates() -> impl IntoResponse {
    let dir = pipeline_template_dir();
    let mut summaries = Vec::new();
    let mut entries = match fs::read_dir(&dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Json(summaries).into_response();
        }
        Err(err) => return map_template_io_error(err, "failed to read template directory"),
    };

    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => return map_template_io_error(err, "failed to read template entry"),
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let template_id = match path.file_stem().and_then(|stem| stem.to_str()).and_then(normalize_template_id) {
            Some(id) => id,
            None => continue,
        };
        let data = match fs::read_to_string(&path).await {
            Ok(data) => data,
            Err(err) => return map_template_io_error(err, "failed to read template file"),
        };
        let Ok(raw) = PipelineTemplateDocumentRaw::decode_str(&data) else {
            continue;
        };
        summaries.push(template_raw_into_summary(template_id, raw));
    }

    summaries.sort_by(|a, b| a.name.cmp(&b.name));
    Json(summaries).into_response()
}

#[utoipa::path(
    get,
    path = "/pipelines/templates/{id}",
    tag = "Pipelines",
    params(("id" = String, Path, description = "Template identifier")),
    responses((status = 200, description = "Template graph document", body = PipelineTemplateDocument), (status = 404, description = "Template not found", body = PipelineError))
)]
async fn fetch_template(State(state): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    let Some(template_id) = normalize_template_id(&id) else {
        return (StatusCode::BAD_REQUEST, Json(PipelineError { error: "invalid template id".into() })).into_response();
    };
    let path = pipeline_template_dir().join(format!("{template_id}.json"));
    let data = match fs::read_to_string(&path).await {
        Ok(data) => data,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (StatusCode::NOT_FOUND, Json(PipelineError { error: "template not found".into() })).into_response();
        }
        Err(err) => return map_template_io_error(err, "failed to read template file"),
    };
    let raw = match PipelineTemplateDocumentRaw::decode_str(&data) {
        Ok(raw) => raw,
        Err(err) => {
            return map_template_io_error(io::Error::new(io::ErrorKind::InvalidData, err), "failed to decode template");
        }
    };
    let mut doc = template_raw_into_document(template_id, raw);
    inject_cached_port_metadata(&state, &mut doc.graph).await;
    Json(doc).into_response()
}
