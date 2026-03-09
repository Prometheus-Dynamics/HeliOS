use axum::{
    Json, Router,
    extract::Path,
    extract::State,
    http::{HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io;
use std::sync::OnceLock;
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AppState;
use super::identity_tokens;
use super::storage;
use crate::http::streams::types::EngineErrorBody;
use crate::http::streams::util::{engine_error_body, map_client_error, normalize_pipeline_manifest};
use crate::http::{streams, streams_persist};
use helios_engine::ipc::NodeRegistrySnapshot;
use helios_engine::ipc::{EngineErrorCode, EngineEvent, StreamManifest};

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

pub(crate) fn normalize_graph_metadata(graph: &mut JsonValue) {
    let Some(obj) = graph.as_object_mut() else { return };
    let Some(values) = obj.get_mut("metadata") else { return };
    let Some(values_obj) = values.as_object_mut() else { return };

    for (_, value) in values_obj.iter_mut() {
        if let JsonValue::Object(map) = value
            && map.contains_key("type")
            && map.contains_key("value")
        {
            // Legacy typed payloads are left as-is; metadata now expects plain JSON.
            continue;
        }
    }
}

fn edge_signature(edge: &JsonValue) -> Option<String> {
    let from = edge.get("from")?;
    let to = edge.get("to")?;
    let from_node = from.get("node")?;
    let from_port = from.get("port")?;
    let to_node = to.get("node")?;
    let to_port = to.get("port")?;
    let from_node = from_node.as_i64().map(|n| n.to_string()).or_else(|| from_node.as_str().map(|s| s.to_string()))?;
    let to_node = to_node.as_i64().map(|n| n.to_string()).or_else(|| to_node.as_str().map(|s| s.to_string()))?;
    let from_port = from_port.as_str()?;
    let to_port = to_port.as_str()?;
    Some(format!("{from_node}:{from_port}->{to_node}:{to_port}"))
}

pub(crate) fn merge_edge_metadata(raw_graph: &JsonValue, graph_json: &mut JsonValue) {
    let Some(raw_edges) = raw_graph.get("edges").and_then(|edges| edges.as_array()) else {
        return;
    };
    let Some(out_edges) = graph_json.get_mut("edges").and_then(|edges| edges.as_array_mut()) else {
        return;
    };

    let mut raw_meta = HashMap::new();
    let mut raw_meta_by_index = Vec::new();
    for edge in raw_edges {
        let signature = edge_signature(edge);
        let metadata = edge.get("metadata").cloned();
        raw_meta_by_index.push((signature.clone(), metadata.clone()));
        if let (Some(signature), Some(metadata)) = (signature, metadata) {
            raw_meta.insert(signature, metadata);
        }
    }

    let mut applied = 0;
    for (idx, edge) in out_edges.iter_mut().enumerate() {
        let signature = edge_signature(edge);
        let metadata = signature.as_ref().and_then(|sig| raw_meta.get(sig)).cloned();
        let metadata = metadata.or_else(|| raw_meta_by_index.get(idx).and_then(|(_, meta)| meta.as_ref()).cloned());
        let Some(metadata) = metadata else { continue };
        if let JsonValue::Object(out_edge) = edge {
            out_edge.insert("metadata".to_string(), metadata);
            applied += 1;
        }
    }

    if applied == 0 && !raw_meta_by_index.is_empty() && raw_meta.is_empty() {
        // Nothing matched by signature; fall back to index-based metadata transfer only.
        for (idx, edge) in out_edges.iter_mut().enumerate() {
            let Some((_, Some(metadata))) = raw_meta_by_index.get(idx) else { continue };
            if let JsonValue::Object(out_edge) = edge {
                out_edge.insert("metadata".to_string(), metadata.clone());
            }
        }
    }
}

fn normalize_backend_id(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    match lower.split_once('@') {
        Some((base, _)) => base.to_string(),
        None => lower,
    }
}

fn build_registry_port_metadata_lookup(snapshot: &NodeRegistrySnapshot) -> HashMap<String, BTreeMap<String, serde_json::Value>> {
    let mut out = HashMap::new();
    for node in &snapshot.nodes {
        let key = normalize_backend_id(&node.id);
        let mut meta: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for (k, v) in &node.metadata {
            if k.starts_with("inputs.") || k.starts_with("outputs.") || k == "daedalus.embedded_graph" || k == "daedalus.embedded_host" || k == "daedalus.embedded_group" {
                // Keep Daedalus metadata values in their adjacently-tagged representation so that
                // graphs can be round-tripped back into the planner/runtime without losing type
                // information.
                meta.insert(k.clone(), serde_json::to_value(v.clone()).unwrap_or(serde_json::Value::Null));
            }
        }
        if !meta.is_empty() {
            out.insert(key, meta);
        }
    }
    out
}

pub(crate) fn inject_port_metadata(graph: &mut serde_json::Value, registry: &NodeRegistrySnapshot) {
    let nodes = match graph.get_mut("nodes").and_then(|v| v.as_array_mut()) {
        Some(nodes) => nodes,
        None => return,
    };
    let lookup = build_registry_port_metadata_lookup(registry);
    if lookup.is_empty() {
        return;
    }
    for node in nodes {
        let id = node.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let normalized = normalize_backend_id(id);
        let Some(meta) = lookup.get(&normalized) else {
            continue;
        };
        let Some(node_obj) = node.as_object_mut() else {
            continue;
        };
        let metadata = node_obj.entry("metadata").or_insert_with(|| serde_json::json!({}));
        let Some(metadata_obj) = metadata.as_object_mut() else {
            continue;
        };
        for (key, value) in meta {
            metadata_obj.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
}

pub(crate) fn inject_pipeline_alias_metadata(graph: &mut serde_json::Value, name: Option<&str>) {
    let Some(name) = name.map(str::trim).filter(|v| !v.is_empty()) else {
        return;
    };
    let Some(obj) = graph.as_object_mut() else {
        return;
    };
    let metadata = obj.entry("metadata").or_insert_with(|| serde_json::json!({}));
    let Some(meta_obj) = metadata.as_object_mut() else {
        return;
    };
    meta_obj.insert("helios.pipeline.alias".to_string(), serde_json::Value::String(name.to_string()));

    // Graph metadata is now stored in `metadata` only.
}

fn pipeline_graph_alias(graph: &serde_json::Value) -> Option<&str> {
    graph
        .get("metadata")
        .and_then(|meta| meta.as_object())
        .and_then(|meta| meta.get("helios.pipeline.alias").and_then(|v| v.as_str()).or_else(|| meta.get("pipeline_alias").and_then(|v| v.as_str())))
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn pipeline_identity_token_set_with_alias(id: Uuid, name: Option<&str>, graph_alias: Option<&str>) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    let id_tok = id.to_string();
    out.insert(id_tok.clone());

    if let Some(name) = name
        && let Some(tok) = identity_tokens::normalize_token(name)
    {
        // A pipeline name is effectively an alias; it must not collide with the UUID token.
        if tok == id_tok {
            return Err(tok);
        }
        let _ = out.insert(tok);
    }

    if let Some(alias) = graph_alias
        && let Some(tok) = identity_tokens::normalize_token(alias)
    {
        // The graph alias is also an alias; it must not collide with the UUID token.
        if tok == id_tok {
            return Err(tok);
        }
        // Allow alias == name (same logical identity), but still include it in the set.
        let _ = out.insert(tok);
    }

    Ok(out)
}

#[derive(Debug, Clone)]
struct PipelineIdentityConflict {
    token: String,
    existing_id: Option<Uuid>,
    existing_name: Option<String>,
    existing_alias: Option<String>,
}

async fn pipeline_identity_conflict(
    dir: &std::path::Path,
    requested_id: Uuid,
    requested_name: Option<&str>,
    requested_graph: &serde_json::Value,
) -> Result<Option<PipelineIdentityConflict>, axum::response::Response> {
    let requested_alias = pipeline_graph_alias(requested_graph);
    let requested_tokens = match pipeline_identity_token_set_with_alias(requested_id, requested_name, requested_alias) {
        Ok(set) => set,
        Err(tok) => {
            return Ok(Some(PipelineIdentityConflict { token: tok, existing_id: None, existing_name: None, existing_alias: None }));
        }
    };

    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) => return Err(map_io_error(err, "failed to read pipeline directory")),
    };

    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => return Err(map_io_error(err, "failed to read pipeline entry")),
        };

        let meta = match entry.metadata().await {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => continue,
            Err(err) => return Err(map_io_error(err, "failed to stat pipeline file")),
        };

        // Ignore obviously unrelated files; the API only manages `<uuid>.json`.
        let Some(file_name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        if !file_name.ends_with(".json") {
            continue;
        }

        let data = match fs::read_to_string(entry.path()).await {
            Ok(data) => data,
            Err(err) => return Err(map_io_error(err, "failed to read pipeline file")),
        };
        let Ok(doc) = serde_json::from_str::<PipelineDocument>(&data) else {
            // Skip malformed entries; they were not written by this API.
            continue;
        };
        if doc.id == requested_id {
            // Self update; allow. (File existence is checked by the caller for updates.)
            continue;
        }

        let existing_alias = pipeline_graph_alias(&doc.graph);
        let existing_tokens = match pipeline_identity_token_set_with_alias(doc.id, doc.name.as_deref(), existing_alias) {
            Ok(set) => set,
            Err(tok) => {
                // Keep the doc's tokens as a blocker even if it is internally inconsistent.
                // Identity collisions are dangerous; do not allow new pipelines to reuse them.
                warn!(pipeline_id = %doc.id, token = %tok, "pipeline has colliding identity tokens; treating as reserved for uniqueness checks");
                let mut set = BTreeSet::new();
                set.insert(doc.id.to_string());
                if let Some(name) = doc.name.as_deref().and_then(identity_tokens::normalize_token) {
                    let _ = set.insert(name);
                }
                if let Some(alias) = existing_alias.and_then(identity_tokens::normalize_token) {
                    let _ = set.insert(alias);
                }
                set
            }
        };

        if let Some(tok) = requested_tokens.intersection(&existing_tokens).next().cloned() {
            // Fallback to file mod time if the stored value is zero (helps debug stale docs).
            let _ = meta;
            return Ok(Some(PipelineIdentityConflict { token: tok, existing_id: Some(doc.id), existing_name: doc.name.clone(), existing_alias: existing_alias.map(|s| s.to_string()) }));
        }
    }

    Ok(None)
}

async fn ensure_unique_pipeline_identity(dir: &std::path::Path, requested_id: Uuid, requested_name: Option<&str>, requested_graph: &serde_json::Value) -> Option<axum::response::Response> {
    match pipeline_identity_conflict(dir, requested_id, requested_name, requested_graph).await {
        Ok(None) => None,
        Ok(Some(conflict)) => {
            if conflict.existing_id.is_none() {
                return Some(
                    (StatusCode::CONFLICT, Json(PipelineError { error: format!("pipeline identity tokens collide within the requested pipeline: token=\"{}\"", conflict.token) })).into_response(),
                );
            }

            let existing_name = conflict.existing_name.as_deref().unwrap_or("");
            let existing_alias = conflict.existing_alias.as_deref().unwrap_or("");
            let mut details = Vec::new();
            if !existing_name.is_empty() {
                details.push(format!("name=\"{existing_name}\""));
            }
            if !existing_alias.is_empty() && existing_alias != existing_name {
                details.push(format!("alias=\"{existing_alias}\""));
            }
            let details = if details.is_empty() { "".to_string() } else { format!(" ({})", details.join(", ")) };
            let existing_id = conflict.existing_id.expect("existing pipeline id set");
            Some(
                (
                    StatusCode::CONFLICT,
                    Json(PipelineError { error: format!("pipeline identity token already in use: token=\"{}\" conflicts with pipeline id=\"{}\"{details}", conflict.token, existing_id) }),
                )
                    .into_response(),
            )
        }
        Err(resp) => Some(resp),
    }
}

#[cfg(test)]
mod identity_tests {
    use super::{pipeline_graph_alias, pipeline_identity_token_set_with_alias};
    use uuid::Uuid;

    #[test]
    fn pipeline_name_cannot_collide_with_id_token() {
        let id = Uuid::new_v4();
        let name = id.to_string().to_uppercase();
        let err = pipeline_identity_token_set_with_alias(id, Some(&name), None).unwrap_err();
        assert_eq!(err, id.to_string());
    }

    #[test]
    fn pipeline_graph_alias_participates_in_uniqueness() {
        let id = Uuid::new_v4();
        let graph = serde_json::json!({
            "nodes": [],
            "edges": [],
            "metadata": { "helios.pipeline.alias": "My Pipeline" }
        });
        let alias = pipeline_graph_alias(&graph).unwrap();
        assert_eq!(alias, "My Pipeline");

        let set = pipeline_identity_token_set_with_alias(id, None, Some(alias)).expect("token set");
        assert!(set.contains(&id.to_string()));
        assert!(set.contains("mypipeline"));
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PipelineDocument {
    pub id: Uuid,
    #[serde(default)]
    pub name: Option<String>,
    pub graph: serde_json::Value,
    pub updated_at_ms: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
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
struct PipelineTemplateDocumentRaw {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    graph: serde_json::Value,
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

#[derive(Clone, Debug)]
struct GraphValidationState {
    diagnostics: Vec<PlannerDiagnostic>,
    updated_at_ms: i64,
}

const GRAPH_VALIDATION_CACHE_MAX_AGE_MS: i64 = 60_000;

fn now_timestamp_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn graph_validation_state_stale(state: &GraphValidationState, graph_updated_at_ms: i64, now_ms: i64) -> bool {
    if graph_updated_at_ms > 0 && state.updated_at_ms < graph_updated_at_ms {
        return true;
    }
    now_ms.saturating_sub(state.updated_at_ms) > GRAPH_VALIDATION_CACHE_MAX_AGE_MS
}

fn graph_validation_cache() -> &'static RwLock<HashMap<Uuid, GraphValidationState>> {
    static CACHE: OnceLock<RwLock<HashMap<Uuid, GraphValidationState>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn map_planner_diagnostics(diagnostics: Vec<helios_engine::ipc::PlannerDiagnostic>) -> Vec<PlannerDiagnostic> {
    diagnostics
        .into_iter()
        .map(|diag| PlannerDiagnostic { code: diag.code, message: diag.message, span: PlannerDiagnosticSpan { pass: diag.span.pass, node: diag.span.node, port: diag.span.port } })
        .collect()
}

async fn set_graph_validation_state(graph_id: Uuid, diagnostics: Vec<PlannerDiagnostic>) {
    let mut cache = graph_validation_cache().write().await;
    cache.insert(graph_id, GraphValidationState { diagnostics, updated_at_ms: now_timestamp_ms() });
}

async fn set_graph_validation_error(graph_id: Uuid, code: Option<EngineErrorCode>, message: String) {
    let fallback = code.map(|code| format!("{code:?}")).unwrap_or_else(|| "validation_error".to_string());
    let diag = PlannerDiagnostic { code: fallback, message, span: PlannerDiagnosticSpan { pass: "engine".to_string(), node: None, port: None } };
    set_graph_validation_state(graph_id, vec![diag]).await;
}

async fn clear_graph_validation_state(graph_id: Uuid) {
    let mut cache = graph_validation_cache().write().await;
    cache.remove(&graph_id);
}

async fn prune_graph_validation_cache(valid_ids: &HashSet<Uuid>) {
    let mut cache = graph_validation_cache().write().await;
    cache.retain(|id, _| valid_ids.contains(id));
}

async fn snapshot_graph_validation() -> HashMap<Uuid, GraphValidationState> {
    graph_validation_cache().read().await.clone()
}

pub(crate) async fn refresh_graph_validation(state: &AppState, graph_id: Uuid, graph: &serde_json::Value) {
    match state.engine.validate_graph_event(graph.clone(), Vec::new(), true).await {
        Ok(EngineEvent::GraphValidation { report, .. }) => {
            let diagnostics = map_planner_diagnostics(report.diagnostics);
            set_graph_validation_state(graph_id, diagnostics).await;
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            set_graph_validation_error(graph_id, Some(code), reason).await;
        }
        Ok(_) => {
            set_graph_validation_error(graph_id, Some(EngineErrorCode::Internal), "unexpected engine response".to_string()).await;
        }
        Err(err) => {
            warn!(error = %err, graph_id = %graph_id, "graph validation failed");
            set_graph_validation_error(graph_id, Some(EngineErrorCode::Internal), format!("validation failed: {err}")).await;
        }
    }
}

#[utoipa::path(
    get,
    path = "/pipelines/registry",
    tag = "Pipelines",
    responses((status = 200, description = "Daedalus node registry", body = DaedalusRegistryResponse))
)]
async fn list_registry(State(state): State<AppState>) -> impl IntoResponse {
    let (snapshot, stale) = match get_cached_registry_snapshot(&state).await {
        Some(value) => value,
        None => {
            return (StatusCode::BAD_GATEWAY, Json(PipelineError { error: "registry unavailable".to_string() })).into_response();
        }
    };

    let mut nodes: Vec<DaedalusRegistryNode> = snapshot
        .nodes
        .into_iter()
        .map(|node| DaedalusRegistryNode {
            id: node.id,
            label: node.label,
            plugin: node.plugin,
            feature_flags: node.feature_flags,
            sync_groups: node
                .sync_groups
                .into_iter()
                .map(|group| DaedalusSyncGroup { name: group.name, policy: group.policy, ports: group.ports, capacity: group.capacity, backpressure: group.backpressure })
                .collect(),
            inputs: node.inputs,
            outputs: node.outputs,
            input_ports: node
                .input_ports
                .into_iter()
                .map(|port| DaedalusRegistryPort { name: port.name, ty: Some(port.ty.into()), source: port.source, const_value: port.const_value.map(|value| value.into()) })
                .collect(),
            fanin_inputs: node.fanin_inputs.into_iter().map(|port| DaedalusRegistryFanInPort { prefix: port.prefix, start: port.start, ty: port.ty.into() }).collect(),
            output_ports: node
                .output_ports
                .into_iter()
                .map(|port| DaedalusRegistryPort { name: port.name, ty: Some(port.ty.into()), source: port.source, const_value: port.const_value.map(|value| value.into()) })
                .collect(),
            default_compute: node.default_compute,
            metadata: node.metadata.into_iter().map(|(k, v)| (k, v.into())).collect(),
        })
        .collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let mut types: Vec<DaedalusRegistryType> = snapshot.types.into_iter().map(|entry| DaedalusRegistryType { rust: entry.rust, ty: entry.ty.into() }).collect();
    types.sort_by(|a, b| a.rust.cmp(&b.rust));

    let mut response = Json(DaedalusRegistryResponse { plugins: snapshot.plugins, nodes, types }).into_response();
    if stale {
        response.headers_mut().insert("x-helios-registry-stale", HeaderValue::from_static("1"));
    }
    response
}

pub async fn warm_registry_cache(state: AppState) {
    let _ = get_cached_registry_snapshot(&state).await;
}

#[derive(Clone)]
struct RegistryCacheEntry {
    fetched_at: Instant,
    snapshot: NodeRegistrySnapshot,
}

fn registry_cache() -> &'static RwLock<Option<RegistryCacheEntry>> {
    static CACHE: OnceLock<RwLock<Option<RegistryCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn registry_refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn get_cached_registry_snapshot(state: &AppState) -> Option<(NodeRegistrySnapshot, bool)> {
    // Registry is large but changes rarely; keep it hot so UI interactions don't stall on engine IPC.
    const FRESH_FOR: Duration = Duration::from_secs(30);
    const IPC_TIMEOUT: Duration = Duration::from_secs(6);
    const IPC_RETRY_TIMEOUT: Duration = Duration::from_secs(18);

    if let Some(entry) = registry_cache().read().await.clone()
        && entry.fetched_at.elapsed() < FRESH_FOR
    {
        return Some((entry.snapshot, false));
    }

    let _refresh_guard = registry_refresh_lock().lock().await;
    if let Some(entry) = registry_cache().read().await.clone()
        && entry.fetched_at.elapsed() < FRESH_FOR
    {
        return Some((entry.snapshot, false));
    }

    let mut stale_entry = registry_cache().read().await.clone();
    match state.engine.get_node_registry_with_timeout(IPC_TIMEOUT).await {
        Ok(snapshot) => {
            *registry_cache().write().await = Some(RegistryCacheEntry { fetched_at: Instant::now(), snapshot: snapshot.clone() });
            Some((snapshot, false))
        }
        Err(error) => {
            if stale_entry.is_none() {
                warn!(
                    error = ?error,
                    timeout_ms = IPC_TIMEOUT.as_millis(),
                    retry_timeout_ms = IPC_RETRY_TIMEOUT.as_millis(),
                    "node registry fetch timed out; retrying with relaxed timeout"
                );
                match state.engine.get_node_registry_with_timeout(IPC_RETRY_TIMEOUT).await {
                    Ok(snapshot) => {
                        *registry_cache().write().await = Some(RegistryCacheEntry { fetched_at: Instant::now(), snapshot: snapshot.clone() });
                        return Some((snapshot, false));
                    }
                    Err(retry_error) => {
                        warn!(
                            error = ?retry_error,
                            timeout_ms = IPC_RETRY_TIMEOUT.as_millis(),
                            "node registry fetch failed after retry"
                        );
                    }
                }
                stale_entry = registry_cache().read().await.clone();
            }
            stale_entry.map(|entry| (entry.snapshot, true))
        }
    }
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
    let report = match state.engine.validate_graph_event(payload.graph, payload.active_features, payload.enable_lints).await {
        Ok(EngineEvent::GraphValidation { report, .. }) => report,
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            if let Some(graph_id) = payload.graph_id {
                set_graph_validation_error(graph_id, Some(code), reason.clone()).await;
            }
            return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(code), reason))).into_response();
        }
        Ok(_) => {
            if let Some(graph_id) = payload.graph_id {
                set_graph_validation_error(graph_id, Some(EngineErrorCode::Internal), "unexpected engine response".to_string()).await;
            }
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response();
        }
        Err(err) => {
            if let Some(graph_id) = payload.graph_id {
                set_graph_validation_error(graph_id, Some(EngineErrorCode::Internal), format!("validation failed: {err}")).await;
            }
            return map_client_error(err);
        }
    };

    let helios_engine::ipc::GraphValidationReport { ok, diagnostics: raw_diagnostics, gpu_segments, gpu_edges, node_ids } = report;
    let diagnostics = map_planner_diagnostics(raw_diagnostics);
    if let Some(graph_id) = payload.graph_id {
        set_graph_validation_state(graph_id, diagnostics.clone()).await;
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

pub(crate) fn normalize_graph_node_ids(graph: &mut daedalus::planner::Graph) {
    for node in &mut graph.nodes {
        if let Some(normalized) = normalize_node_id(&node.id.0) {
            node.id.0 = normalized;
        }
    }
}

fn normalize_node_id(id: &str) -> Option<String> {
    let mut parts = id.split(':').filter(|part| !part.is_empty());
    let root = parts.next()?;
    let rest: Vec<&str> = parts.filter(|part| *part != root).collect();
    if rest.is_empty() {
        return None;
    }
    let normalized = std::iter::once(root).chain(rest).collect::<Vec<_>>().join(":");
    if normalized == id { None } else { Some(normalized) }
}

fn unwrap_pipeline_export_graph(payload: &serde_json::Value) -> Option<serde_json::Value> {
    let obj = payload.as_object()?;
    if let Some(graph) = obj.get("graph").and_then(|g| g.as_object()).cloned() {
        return Some(serde_json::Value::Object(graph));
    }
    let pipeline = obj.get("pipeline")?.as_object()?;
    let graph = pipeline.get("graph")?.as_object()?.clone();
    Some(serde_json::Value::Object(graph))
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

    // The legacy pipeline graph format has been retired; only Daedalus graphs are accepted.
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
    if let Ok(snapshot) = state.engine.get_node_registry().await {
        inject_port_metadata(&mut graph_json, &snapshot);
        merge_edge_metadata(&payload.graph, &mut graph_json);
    } else {
        merge_edge_metadata(&payload.graph, &mut graph_json);
    }

    let _guard = pipeline_graph_write_lock().lock().await;
    let id = Uuid::new_v4();

    // If the client did not provide a pipeline name, use the graph's stored pipeline alias.
    if !explicit_name_provided && let Some(alias) = pipeline_graph_alias(&graph_json) {
        name = Some(alias.to_string());
    }

    // Keep the graph metadata and stored doc name aligned when we have a name.
    inject_pipeline_alias_metadata(&mut graph_json, name.as_deref());

    // Auto-rename derived names on create to avoid collisions.
    if !explicit_name_provided {
        const MAX_ATTEMPTS: usize = 100;
        let base = name.clone().unwrap_or_default();
        let base = base.trim().to_string();
        if !base.is_empty() {
            let mut selected: Option<String> = None;
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
    let doc = PipelineDocument { id, name, graph: graph_json, updated_at_ms: chrono::Utc::now().timestamp_millis() };
    let path = dir.join(format!("{id}.json"));
    let data = match serde_json::to_vec_pretty(&doc) {
        Ok(bytes) => bytes,
        Err(err) => return (StatusCode::BAD_REQUEST, Json(PipelineError { error: format!("invalid graph payload: {err}") })).into_response(),
    };
    match fs::write(path, data).await {
        Ok(_) => {
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
        Ok(_) => return (StatusCode::NOT_FOUND, Json(PipelineError { error: "graph not found".into() })).into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return (StatusCode::NOT_FOUND, Json(PipelineError { error: "graph not found".into() })).into_response();
        }
        Err(err) => return map_io_error(err, "failed to stat graph"),
    }

    // Preserve the previous name when the client does not send one and the graph has no alias.
    let existing_doc = match fs::read_to_string(&path).await {
        Ok(data) => serde_json::from_str::<PipelineDocument>(&data).ok(),
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
    if let Ok(snapshot) = state.engine.get_node_registry().await {
        inject_port_metadata(&mut graph_json, &snapshot);
        merge_edge_metadata(&payload.graph, &mut graph_json);
    } else {
        merge_edge_metadata(&payload.graph, &mut graph_json);
    }

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
    let doc = PipelineDocument { id, name, graph: graph_json, updated_at_ms: chrono::Utc::now().timestamp_millis() };
    let data = match serde_json::to_vec_pretty(&doc) {
        Ok(bytes) => bytes,
        Err(err) => return (StatusCode::BAD_REQUEST, Json(PipelineError { error: format!("invalid graph payload: {err}") })).into_response(),
    };
    match fs::write(path, data).await {
        Ok(_) => {
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
async fn list_graphs(State(state): State<AppState>) -> impl IntoResponse {
    let dir = match pipeline_dir() {
        Ok(dir) => dir,
        Err(resp) => return *resp,
    };
    let mut loaded_docs: Vec<(PipelineDocument, i64)> = Vec::new();
    let mut existing_ids = HashSet::new();
    let mut validation_snapshot = snapshot_graph_validation().await;
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) => return map_io_error(err, "failed to read pipeline directory"),
    };

    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => return map_io_error(err, "failed to read pipeline entry"),
        };

        let meta = match entry.metadata().await {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => continue,
            Err(err) => return map_io_error(err, "failed to stat pipeline file"),
        };

        let data = match fs::read_to_string(entry.path()).await {
            Ok(data) => data,
            Err(err) => return map_io_error(err, "failed to read pipeline file"),
        };
        if let Ok(doc) = serde_json::from_str::<PipelineDocument>(&data) {
            let doc_id = doc.id;
            let mut updated_at_ms = doc.updated_at_ms.max(0);
            // Fallback to file modification time if the stored value is zero.
            if updated_at_ms == 0
                && let Ok(modified) = meta.modified()
                && let Ok(ts) = modified.duration_since(std::time::UNIX_EPOCH)
            {
                updated_at_ms = ts.as_millis() as i64;
            }
            loaded_docs.push((doc, updated_at_ms));
            existing_ids.insert(doc_id);
        } else {
            // Skip malformed entries; they were not written by this API.
            continue;
        }
    }

    let now_ms = now_timestamp_ms();
    for (doc, updated_at_ms) in &loaded_docs {
        let needs_refresh = match validation_snapshot.get(&doc.id) {
            Some(state) => graph_validation_state_stale(state, *updated_at_ms, now_ms),
            None => true,
        };
        if needs_refresh {
            refresh_graph_validation(&state, doc.id, &doc.graph).await;
        }
    }
    if !loaded_docs.is_empty() {
        validation_snapshot = snapshot_graph_validation().await;
    }

    let summaries: Vec<PipelineSummary> = loaded_docs
        .into_iter()
        .map(|(doc, updated_at_ms)| {
            let issue_count = validation_snapshot.get(&doc.id).map(|state| state.diagnostics.len()).unwrap_or(0);
            PipelineSummary { id: doc.id, name: doc.name, updated_at_ms, issue_count }
        })
        .collect();

    prune_graph_validation_cache(&existing_ids).await;
    Json(summaries).into_response()
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
    match fs::read_to_string(&path).await {
        Ok(data) => match serde_json::from_str::<PipelineDocument>(&data) {
            Ok(mut doc) => {
                if let Ok(snapshot) = state.engine.get_node_registry().await {
                    inject_port_metadata(&mut doc.graph, &snapshot);
                }
                Json(doc).into_response()
            }
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, Json(PipelineError { error: format!("failed to decode graph: {err}") })).into_response(),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (StatusCode::NOT_FOUND, Json(PipelineError { error: "graph not found".into() })).into_response(),
        Err(err) => map_io_error(err, "failed to read graph"),
    }
}

fn manifest_references_pipeline(manifest: &StreamManifest, pipeline_id: Uuid) -> bool {
    if manifest.active_pipeline_id == Some(pipeline_id) {
        return true;
    }
    if manifest.pipelines.iter().any(|binding| binding.pipeline_id == pipeline_id) {
        return true;
    }
    if let Some(layout) = manifest.pipeline_layout.as_ref() {
        return layout.slots.iter().any(|slot| slot.pipeline_id == Some(pipeline_id));
    }
    false
}

fn detach_pipeline_from_manifest(manifest: &mut StreamManifest, pipeline_id: Uuid) -> bool {
    if !manifest_references_pipeline(manifest, pipeline_id) {
        return false;
    }

    let mut changed = false;

    let before_pipelines = manifest.pipelines.len();
    manifest.pipelines.retain(|binding| binding.pipeline_id != pipeline_id);
    if manifest.pipelines.len() != before_pipelines {
        changed = true;
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        let before_slots = layout.slots.len();
        layout.slots.retain(|slot| slot.pipeline_id != Some(pipeline_id));
        if layout.slots.len() != before_slots {
            changed = true;
        }
    }

    if manifest.active_pipeline_id == Some(pipeline_id) {
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        changed = true;
    }

    if let Some(active_id) = manifest.active_pipeline_id
        && !manifest.pipelines.iter().any(|binding| binding.pipeline_id == active_id)
    {
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        changed = true;
    }

    if manifest.active_pipeline_id.is_none() && !manifest.pipelines.is_empty() {
        let next_active = manifest.pipelines[0].pipeline_id;
        manifest.active_pipeline_id = Some(next_active);
        let output = manifest.pipelines.iter().find(|binding| binding.pipeline_id == next_active).and_then(|binding| binding.pipeline_output.clone());
        manifest.active_pipeline_output = output;
        changed = true;
    }

    if manifest.pipelines.is_empty() {
        manifest.pipeline_enabled = Some(false);
        manifest.pipeline_layout = None;
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        changed = true;
    } else if manifest.pipeline_enabled == Some(false) {
        manifest.pipeline_enabled = Some(true);
        changed = true;
    }

    normalize_pipeline_manifest(manifest);

    changed
}

async fn detach_pipeline_from_streams(state: &AppState, pipeline_id: Uuid) -> Result<(), String> {
    let mut updated_streams: HashSet<Uuid> = HashSet::new();
    let running = match state.engine.list_streams().await {
        Ok(list) => list,
        Err(err) => {
            warn!(pipeline_id = %pipeline_id, error = %err, "failed to list running streams while detaching pipeline");
            Vec::new()
        }
    };

    for stream in running {
        if stream.manifest.internal {
            continue;
        }
        let mut manifest = stream.manifest.clone();
        if !detach_pipeline_from_manifest(&mut manifest, pipeline_id) {
            continue;
        }
        manifest.identity.id = Some(stream.stream_id);
        if let Err(status) = streams::restart_stream_with_manifest(state.clone(), manifest).await {
            warn!(pipeline_id = %pipeline_id, stream_id = %stream.stream_id, status = %status, "failed to restart stream after detaching pipeline");
        }
        updated_streams.insert(stream.stream_id);
    }

    let records = streams_persist::list_persisted_records().await;
    for record in records {
        let Some(mut manifest) = record.manifest else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let stream_id = manifest.identity.id.or(record.last_stream_id).unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        if updated_streams.contains(&stream_id) {
            continue;
        }
        if !detach_pipeline_from_manifest(&mut manifest, pipeline_id) {
            continue;
        }
        manifest.identity.id = Some(stream_id);
        streams_persist::persist_manifest_checked(&record.camera_id, Some(stream_id), manifest)
            .await
            .map_err(|err| format!("deleted graph but failed to persist detached stream manifest for {}: {err}", record.camera_id))?;
    }

    Ok(())
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
            clear_graph_validation_state(id).await;
            match detach_pipeline_from_streams(&state, id).await {
                Ok(()) => StatusCode::NO_CONTENT.into_response(),
                Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, Json(PipelineError { error: err })).into_response(),
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (StatusCode::NOT_FOUND, Json(PipelineError { error: "graph not found".into() })).into_response(),
        Err(err) => map_io_error(err, "failed to delete graph"),
    }
}

fn pipeline_dir() -> Result<std::path::PathBuf, Box<axum::response::Response>> {
    storage::ensure_subdir("pipelines").map_err(|err| Box::new(map_io_error(err, "failed to prepare pipeline directory")))
}

pub(crate) fn pipeline_template_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("HELIOS_PIPELINE_TEMPLATE_DIR") {
        return std::path::PathBuf::from(dir);
    }
    if let Ok(cwd) = std::env::current_dir() {
        let dev = cwd.join("configs").join("templates");
        if dev.is_dir() {
            return dev;
        }
    }
    std::path::PathBuf::from("/usr/share/helios/pipeline-templates")
}

fn normalize_template_id(raw: &str) -> Option<String> {
    let sanitized = storage::sanitize_name(raw)?;
    let trimmed = sanitized.trim().trim_end_matches(".json");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

async fn template_exists(raw_id: &str) -> bool {
    let Some(template_id) = normalize_template_id(raw_id) else {
        return false;
    };
    let path = pipeline_template_dir().join(format!("{template_id}.json"));
    match fs::metadata(path).await {
        Ok(meta) => meta.is_file(),
        Err(_) => false,
    }
}

fn map_template_io_error<E: Into<std::io::Error>>(err: E, context: &str) -> axum::response::Response {
    let err = err.into();
    let status = if err.kind() == std::io::ErrorKind::NotFound { StatusCode::NOT_FOUND } else { StatusCode::INTERNAL_SERVER_ERROR };
    (status, Json(PipelineError { error: format!("{context}: {err}") })).into_response()
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
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Json(summaries).into_response(),
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
        let Ok(raw) = serde_json::from_str::<PipelineTemplateDocumentRaw>(&data) else {
            continue;
        };
        let PipelineTemplateDocumentRaw { id, name, summary, tags, graph: _ } = raw;
        let _raw_id = id.as_deref().and_then(normalize_template_id);
        let name = name.unwrap_or_else(|| template_id.clone());
        summaries.push(PipelineTemplateSummary { template_id, name, summary, tags });
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
    let raw = match serde_json::from_str::<PipelineTemplateDocumentRaw>(&data) {
        Ok(raw) => raw,
        Err(err) => return map_template_io_error(io::Error::new(io::ErrorKind::InvalidData, err), "failed to decode template"),
    };
    let PipelineTemplateDocumentRaw { id, name, summary, tags, graph } = raw;
    let _raw_id = id.as_deref().and_then(normalize_template_id);
    let name = name.unwrap_or_else(|| template_id.clone());
    let mut graph = graph;
    if let Some(unwrapped) = unwrap_pipeline_export_graph(&graph) {
        graph = unwrapped;
    }
    normalize_graph_metadata(&mut graph);
    if let Ok(snapshot) = state.engine.get_node_registry().await {
        inject_port_metadata(&mut graph, &snapshot);
    }
    let doc = PipelineTemplateDocument { id: template_id, name, summary, tags, graph };
    Json(doc).into_response()
}

fn map_io_error<E: Into<std::io::Error>>(err: E, context: &str) -> axum::response::Response {
    let err = err.into();
    let status = if err.kind() == std::io::ErrorKind::NotFound { StatusCode::NOT_FOUND } else { StatusCode::INTERNAL_SERVER_ERROR };
    (status, Json(PipelineError { error: format!("{context}: {err}") })).into_response()
}

/// Load a stored pipeline document by id for reuse in other handlers.
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

#[derive(Debug)]
pub(crate) struct PipelineRefreshFailure {
    pub stream_id: Uuid,
    pub error: String,
}

pub(crate) async fn refresh_pipeline_consumers(state: &AppState, pipeline_id: Uuid, graph: &JsonValue) -> Vec<PipelineRefreshFailure> {
    let mut failures = Vec::new();
    let streams = match state.engine.list_streams().await {
        Ok(streams) => streams,
        Err(err) => {
            warn!(pipeline_id = %pipeline_id, error = %err, "failed to list streams for pipeline refresh");
            return vec![PipelineRefreshFailure { stream_id: Uuid::nil(), error: err.to_string() }];
        }
    };

    for stream in streams {
        let manifest = &stream.manifest;
        let uses_pipeline = manifest.pipelines.iter().any(|binding| binding.pipeline_id == pipeline_id);
        if !uses_pipeline {
            continue;
        }
        if let Err(err) = state.engine.set_graph(stream.stream_id, graph.clone(), Some(pipeline_id), None).await {
            warn!(stream_id = %stream.stream_id, pipeline_id = %pipeline_id, error = %err, "failed to refresh pipeline graph on stream");
            failures.push(PipelineRefreshFailure { stream_id: stream.stream_id, error: err.to_string() });
        }
    }

    failures
}

pub(crate) async fn refresh_pipeline_input_consumers(state: &AppState, pipeline_id: Uuid, inputs: BTreeMap<String, Option<JsonValue>>) -> Vec<PipelineRefreshFailure> {
    let mut failures = Vec::new();
    let streams = match state.engine.list_streams().await {
        Ok(streams) => streams,
        Err(err) => {
            warn!(pipeline_id = %pipeline_id, error = %err, "failed to list streams for pipeline input refresh");
            return vec![PipelineRefreshFailure { stream_id: Uuid::nil(), error: err.to_string() }];
        }
    };

    for stream in streams {
        let manifest = &stream.manifest;
        let uses_pipeline = manifest.pipelines.iter().any(|binding| binding.pipeline_id == pipeline_id);
        if !uses_pipeline {
            continue;
        }
        match state.engine.set_pipeline_inputs(stream.stream_id, Some(pipeline_id), inputs.clone()).await {
            Ok(EngineEvent::Ack { .. }) => {}
            Ok(EngineEvent::Nack { code, reason, .. }) => {
                let error = format!("engine rejected inputs: {code:?}: {reason}");
                warn!(stream_id = %stream.stream_id, pipeline_id = %pipeline_id, %error, "failed to refresh pipeline inputs on stream");
                failures.push(PipelineRefreshFailure { stream_id: stream.stream_id, error });
            }
            Ok(other) => {
                let error = format!("unexpected engine response: {other:?}");
                warn!(stream_id = %stream.stream_id, pipeline_id = %pipeline_id, %error, "failed to refresh pipeline inputs on stream");
                failures.push(PipelineRefreshFailure { stream_id: stream.stream_id, error });
            }
            Err(err) => {
                warn!(stream_id = %stream.stream_id, pipeline_id = %pipeline_id, error = %err, "failed to refresh pipeline inputs on stream");
                failures.push(PipelineRefreshFailure { stream_id: stream.stream_id, error: err.to_string() });
            }
        }
    }

    failures
}
