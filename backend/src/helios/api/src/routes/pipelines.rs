//! Pipelines: Daedalus graphs that helios-engine runs, as Orion workloads of runtime
//! `helios.engine.execution.v1`. A pipeline's graph is a versioned `GraphDocument`
//! (`format: "daedalus.graph"`), validated with Daedalus's parser and handed to the engine
//! inline. Names and revisions travel as labels on the pipeline's Orion artifact; earlier
//! revisions are kept by the API for rollback.

use std::collections::{BTreeMap, BTreeSet};

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use daedalus_planner::GraphDocument;
use orion::{
    control_plane::{ArtifactRecord, DesiredState, DesiredStateMutation, ResourceRecord, TypedConfigValue, WorkloadConfig, WorkloadRecord},
    core::{ArtifactId, NodeId, ResourceId, WorkloadId},
};
use serde::{Deserialize, Serialize};

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    host::now_ms,
    orion::{StateView, enum_name, label_map, label_values},
};

use super::check_id;

pub const ENGINE_RUNTIME: &str = "helios.engine.execution.v1";
const ENGINE_CONFIG_SCHEMA: &str = "helios.engine.execution.config.v1";
const GRAPH_CONTENT_TYPE: &str = "application/vnd.daedalus.graph+json";
const WORKLOAD_PREFIX: &str = "pipeline.";
const ARTIFACT_PREFIX: &str = "artifact.pipeline.";
const NAME_LABEL: &str = "helios.pipeline.name";
const REVISION_LABEL: &str = "helios.pipeline.revision";
const SESSION_TYPE: &str = "execution.session";
const ARTIFACT_TYPE: &str = "execution.artifact";
const TELEMETRY_KIND: &str = "execution.telemetry";
const HOST_OUTPUT_PREFIX: &str = "host_output:";

/// Frame-source options for a camera binding (all optional).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub resource_id: String,
    /// Which camera of the service (name, part of it, or an identity key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_height: Option<u32>,
    /// Half-size pyramid levels attached to each frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pyramid: Option<u8>,
}

/// What a client sends to create or replace a pipeline.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PipelineSpec {
    pub name: String,
    /// A versioned Daedalus `GraphDocument`.
    pub graph: serde_json::Value,
    #[serde(default)]
    pub bindings: BTreeMap<String, Binding>,
    /// Desired running (default) or stopped.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePipeline {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(flatten)]
    pub spec: PipelineSpec,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Session {
    pub status: String,
    pub message: Option<String>,
    pub observed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Output {
    pub pipeline: String,
    pub port: String,
    /// The latest value as JSON (frames are described, never sent).
    pub value: serde_json::Value,
    pub observed_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Pipeline {
    pub id: String,
    pub workload_id: String,
    pub name: String,
    pub node_id: Option<String>,
    pub revision: Option<u64>,
    pub enabled: bool,
    /// Orion's observed state: pending, assigned, starting, running, stopped, completed, failed.
    pub state: String,
    pub graph: Option<serde_json::Value>,
    pub graph_error: Option<String>,
    pub bindings: BTreeMap<String, Binding>,
    pub plugins: Vec<String>,
    pub session: Option<Session>,
    /// helios-engine's frame statistics (fps, last tick, frame counts, source status).
    pub telemetry: Option<serde_json::Value>,
    pub outputs: Vec<Output>,
    /// Managed by this API (created through `/v1/pipelines`) rather than written by other tools.
    pub managed: bool,
}

pub fn pipeline_id(workload_id: &str) -> String {
    workload_id.strip_prefix(WORKLOAD_PREFIX).unwrap_or(workload_id).to_string()
}

fn engine_workloads(view: &StateView) -> impl Iterator<Item = &WorkloadRecord> {
    view.workloads.values().filter(|w| w.runtime_type.as_str() == ENGINE_RUNTIME)
}

fn find_workload<'a>(view: &'a StateView, id: &str) -> Option<&'a WorkloadRecord> {
    view.workloads.get(&format!("{WORKLOAD_PREFIX}{id}")).or_else(|| view.workloads.get(id)).filter(|w| w.runtime_type.as_str() == ENGINE_RUNTIME)
}

fn config_string(workload: &WorkloadRecord, key: &str) -> Option<String> {
    workload.config.as_ref().and_then(|c| c.payload.get(key)).and_then(|v| v.as_str()).map(str::to_string)
}

fn config_u64(workload: &WorkloadRecord, key: &str) -> Option<u64> {
    workload.config.as_ref().and_then(|c| c.payload.get(key)).and_then(|v| match v {
        TypedConfigValue::Int(i) => u64::try_from(*i).ok(),
        TypedConfigValue::UInt(u) => Some(*u),
        _ => None,
    })
}

/// Bindings back out of an engine workload's `binding.<input>.*` config fields.
pub fn decode_bindings(workload: &WorkloadRecord) -> BTreeMap<String, Binding> {
    let mut inputs = BTreeSet::new();
    if let Some(config) = &workload.config {
        for key in config.payload.keys() {
            if let Some(rest) = key.strip_prefix("binding.")
                && let Some((input, _)) = rest.split_once('.')
            {
                inputs.insert(input.to_string());
            }
        }
    }
    inputs
        .into_iter()
        .filter_map(|input| {
            let resource_id = config_string(workload, &format!("binding.{input}.resource_id"))?;
            let binding = Binding {
                resource_id,
                camera: config_string(workload, &format!("binding.{input}.camera")),
                output_width: config_u64(workload, &format!("binding.{input}.output_width")).map(|v| v as u32),
                output_height: config_u64(workload, &format!("binding.{input}.output_height")).map(|v| v as u32),
                pyramid: config_u64(workload, &format!("binding.{input}.pyramid")).map(|v| v as u8),
            };
            Some((input, binding))
        })
        .collect()
}

fn workload_resources<'a>(view: &'a StateView, workload_id: &'a str, resource_type: &'a str) -> impl Iterator<Item = &'a ResourceRecord> + 'a {
    view.resources.values().filter(move |r| r.resource_type.as_str() == resource_type && r.realized_for_workload_id.as_ref().is_some_and(|w| w.as_str() == workload_id))
}

fn state_field(record: &ResourceRecord, key: &str) -> Option<String> {
    record.state.as_ref().and_then(|s| s.config.as_ref()).and_then(|c| c.payload.get(key)).and_then(|v| v.as_str()).map(str::to_string)
}

fn parse_message(message: Option<String>) -> serde_json::Value {
    match message {
        Some(text) => serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text)),
        None => serde_json::Value::Null,
    }
}

fn outputs_of(view: &StateView, workload_id: &str) -> Vec<Output> {
    let id = pipeline_id(workload_id);
    workload_resources(view, workload_id, ARTIFACT_TYPE)
        .filter_map(|record| {
            let kind = label_map(&record.labels).get("helios.artifact.kind").cloned().or_else(|| state_field(record, "kind"))?;
            let port = kind.strip_prefix(HOST_OUTPUT_PREFIX)?.to_string();
            Some(Output { pipeline: id.clone(), port, value: parse_message(state_field(record, "message")), observed_at_ms: record.state.as_ref().map(|s| s.observed_at_ms).unwrap_or_default() })
        })
        .collect()
}

pub fn pipeline_dto(view: &StateView, workload: &WorkloadRecord) -> Pipeline {
    let workload_id = workload.workload_id.to_string();
    let artifact = view.artifacts.get(workload.artifact_id.as_str());
    let labels = artifact.map(|a| label_map(&a.labels)).unwrap_or_default();
    let inline = config_string(workload, "graph.inline");
    let (graph, graph_error) = match &inline {
        Some(text) => match serde_json::from_str::<serde_json::Value>(text) {
            Ok(value) => (Some(value), None),
            Err(error) => (None, Some(format!("graph.inline is not JSON: {error}"))),
        },
        None => (None, Some(format!("graph is not inline (graph.kind = {})", config_string(workload, "graph.kind").unwrap_or_else(|| "artifact".into())))),
    };
    let plugins = workload
        .config
        .as_ref()
        .map(|c| c.payload.iter().filter(|(k, _)| k.starts_with("plugin.") && k.ends_with(".name")).filter_map(|(_, v)| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let session = workload_resources(view, &workload_id, SESSION_TYPE).next().map(|record| Session {
        status: state_field(record, "status").unwrap_or_else(|| "unknown".into()),
        message: state_field(record, "message"),
        observed_at_ms: record.state.as_ref().map(|s| s.observed_at_ms).unwrap_or_default(),
    });
    let telemetry = workload_resources(view, &workload_id, ARTIFACT_TYPE)
        .find(|record| label_map(&record.labels).get("helios.artifact.kind").map(String::as_str) == Some(TELEMETRY_KIND))
        .map(|record| parse_message(state_field(record, "message")));
    Pipeline {
        id: pipeline_id(&workload_id),
        name: labels.get(NAME_LABEL).cloned().unwrap_or_else(|| pipeline_id(&workload_id)),
        revision: labels.get(REVISION_LABEL).and_then(|r| r.parse().ok()),
        managed: workload_id.starts_with(WORKLOAD_PREFIX),
        node_id: workload.assigned_node_id.as_ref().map(ToString::to_string),
        enabled: workload.desired_state == DesiredState::Running,
        state: enum_name(workload.observed_state),
        graph,
        graph_error,
        bindings: decode_bindings(workload),
        plugins,
        session,
        telemetry,
        outputs: outputs_of(view, &workload_id),
        workload_id,
    }
}

/// State per pipeline, for change events.
pub fn digest(view: &StateView) -> BTreeMap<String, serde_json::Value> {
    engine_workloads(view)
        .map(|workload| {
            let session = workload_resources(view, workload.workload_id.as_str(), SESSION_TYPE).next().and_then(|r| state_field(r, "status"));
            (
                pipeline_id(workload.workload_id.as_str()),
                serde_json::json!({ "state": enum_name(workload.observed_state), "enabled": workload.desired_state == DesiredState::Running, "session": session }),
            )
        })
        .collect()
}

/// Pipelines whose bindings use `resource_id`.
pub fn pipelines_using(view: &StateView, resource_id: &str) -> Vec<String> {
    engine_workloads(view).filter(|w| decode_bindings(w).values().any(|b| b.resource_id == resource_id)).map(|w| pipeline_id(w.workload_id.as_str())).collect()
}

/// Plugins helios-engine reports as loaded (labels on its runtime resource), when it runs.
pub fn loaded_plugins(view: &StateView, node_id: &str) -> Option<Vec<String>> {
    let runtime = view.resources.get(&format!("engine.runtime.{node_id}"))?;
    Some(label_values(&runtime.labels, "helios.plugin.loaded").into_iter().map(str::to_string).collect())
}

/// Validate a spec against Daedalus's document format and the live state.
pub fn validate_spec(spec: &PipelineSpec, view: &StateView, node_id: &str) -> ApiResult<GraphDocument> {
    if spec.name.trim().is_empty() || spec.name.len() > 120 {
        return Err(ApiError::unprocessable("name must be 1 to 120 characters"));
    }
    let text = serde_json::to_string(&spec.graph).map_err(|error| ApiError::bad_request(error.to_string()))?;
    let document = GraphDocument::from_json(&text).map_err(|error| ApiError::unprocessable(format!("graph is not a valid Daedalus GraphDocument: {error}")))?;
    if let Some(loaded) = loaded_plugins(view, node_id) {
        let missing: Vec<&str> = document.requires.iter().map(|r| r.id.as_str()).filter(|id| !loaded.iter().any(|p| p == id)).collect();
        if !missing.is_empty() {
            return Err(ApiError::unprocessable(format!("graph requires plugins helios-engine has not loaded: {} (loaded: {})", missing.join(", "), loaded.join(", "))));
        }
    }
    for (input, binding) in &spec.bindings {
        if !super::valid_id(input) || input.contains('.') {
            return Err(ApiError::unprocessable(format!("invalid binding input name {input:?}")));
        }
        if !view.resources.contains_key(&binding.resource_id) {
            return Err(ApiError::unprocessable(format!("binding {input} names unknown resource {}", binding.resource_id)));
        }
        if binding.output_width.is_some() != binding.output_height.is_some() || binding.output_width == Some(0) || binding.output_height == Some(0) {
            return Err(ApiError::unprocessable(format!("binding {input}: output_width and output_height are set together and non-zero")));
        }
    }
    Ok(document)
}

/// The Orion records for a pipeline spec.
pub fn records_for(id: &str, spec: &PipelineSpec, document: &GraphDocument, revision: u64, node_id: &str) -> ApiResult<(ArtifactRecord, WorkloadRecord)> {
    let workload_id = format!("{WORKLOAD_PREFIX}{id}");
    let artifact_id = format!("{ARTIFACT_PREFIX}{id}");
    let inline = document.to_json().map_err(|error| ApiError::internal(error.to_string()))?;
    let artifact = ArtifactRecord::builder(ArtifactId::new(artifact_id.clone()))
        .content_type(GRAPH_CONTENT_TYPE)
        .size_bytes(inline.len() as u64)
        .label(format!("{NAME_LABEL}={}", spec.name.replace('\n', " ")))
        .label(format!("{REVISION_LABEL}={revision}"))
        .build();
    let mut config = WorkloadConfig::new(ENGINE_CONFIG_SCHEMA).field("graph.kind", TypedConfigValue::String("inline".into())).field("graph.inline", TypedConfigValue::String(inline));
    // The engine checks `plugin.N.version` for equality, while documents use `>=`; requirements
    // stay by name here and the engine plans against the document's own `requires`.
    for (index, requirement) in document.requires.iter().enumerate() {
        config = config.field(format!("plugin.{index}.name"), TypedConfigValue::String(requirement.id.clone()));
    }
    let mut builder = WorkloadRecord::builder(WorkloadId::new(workload_id), ENGINE_RUNTIME, ArtifactId::new(artifact_id))
        .desired_state(if spec.enabled { DesiredState::Running } else { DesiredState::Stopped })
        .assigned_to(NodeId::new(node_id));
    for (input, binding) in &spec.bindings {
        config = config.field(format!("binding.{input}.resource_id"), TypedConfigValue::String(binding.resource_id.clone()));
        if let Some(camera) = &binding.camera {
            config = config.field(format!("binding.{input}.camera"), TypedConfigValue::String(camera.clone()));
        }
        if let (Some(width), Some(height)) = (binding.output_width, binding.output_height) {
            config = config
                .field(format!("binding.{input}.output_width"), TypedConfigValue::Int(i64::from(width)))
                .field(format!("binding.{input}.output_height"), TypedConfigValue::Int(i64::from(height)));
        }
        if let Some(levels) = binding.pyramid {
            config = config.field(format!("binding.{input}.pyramid"), TypedConfigValue::Int(i64::from(levels)));
        }
        builder = builder.bind_resource(ResourceId::new(binding.resource_id.clone()), NodeId::new(node_id));
    }
    Ok((artifact, builder.config(config).build()))
}

async fn deploy(state: &SharedState, id: &str, spec: PipelineSpec) -> ApiResult<Pipeline> {
    let view = state.orion.view().await?;
    let document = validate_spec(&spec, &view, &state.config.node_id)?;
    let revision = state.store.history(id).await?.next_revision();
    let (artifact, workload) = records_for(id, &spec, &document, revision, &state.config.node_id)?;
    state.orion.apply(vec![DesiredStateMutation::PutArtifact(artifact), DesiredStateMutation::PutWorkload(workload)]).await?;
    let spec_json = serde_json::to_value(&spec).map_err(|error| ApiError::internal(error.to_string()))?;
    state.store.push_revision(id, spec_json, now_ms()).await?;
    fetch(state, id).await
}

async fn fetch(state: &SharedState, id: &str) -> ApiResult<Pipeline> {
    let view = state.orion.view().await?;
    find_workload(&view, id).map(|w| pipeline_dto(&view, w)).ok_or_else(|| ApiError::not_found(format!("no pipeline {id}")))
}

pub async fn list(State(state): State<SharedState>) -> ApiResult<Json<Vec<Pipeline>>> {
    let view = state.orion.view().await?;
    Ok(Json(engine_workloads(&view).map(|w| pipeline_dto(&view, w)).collect()))
}

pub async fn get_one(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<Pipeline>> {
    check_id(&id)?;
    fetch(&state, &id).await.map(Json)
}

fn slug(name: &str) -> String {
    let mut out: String = name.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-');
    let out = if out.is_empty() { "pipeline" } else { out };
    out.chars().take(40).collect()
}

pub async fn create(State(state): State<SharedState>, Json(request): Json<CreatePipeline>) -> ApiResult<(StatusCode, Json<Pipeline>)> {
    let id = match request.id {
        Some(id) => id,
        None => format!("{}-{:x}", slug(&request.spec.name), now_ms() & 0xfffff),
    };
    check_id(&id)?;
    let view = state.orion.view().await?;
    if find_workload(&view, &id).is_some() {
        return Err(ApiError::conflict(format!("pipeline {id} exists; PUT /v1/pipelines/{id} replaces it")));
    }
    Ok((StatusCode::CREATED, Json(deploy(&state, &id, request.spec).await?)))
}

pub async fn put(State(state): State<SharedState>, Path(id): Path<String>, Json(spec): Json<PipelineSpec>) -> ApiResult<Json<Pipeline>> {
    check_id(&id)?;
    deploy(&state, &id, spec).await.map(Json)
}

pub async fn remove(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    check_id(&id)?;
    let view = state.orion.view().await?;
    let workload = find_workload(&view, &id).ok_or_else(|| ApiError::not_found(format!("no pipeline {id}")))?;
    let mut mutations = vec![DesiredStateMutation::RemoveWorkload(workload.workload_id.clone())];
    if workload.artifact_id.as_str().starts_with(ARTIFACT_PREFIX) {
        mutations.push(DesiredStateMutation::RemoveArtifact(workload.artifact_id.clone()));
    }
    state.orion.apply(mutations).await?;
    state.store.forget(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn set_desired(state: &SharedState, id: &str, desired: DesiredState) -> ApiResult<Pipeline> {
    check_id(id)?;
    let view = state.orion.view().await?;
    let mut workload = find_workload(&view, id).cloned().ok_or_else(|| ApiError::not_found(format!("no pipeline {id}")))?;
    workload.desired_state = desired;
    state.orion.apply(vec![DesiredStateMutation::PutWorkload(workload)]).await?;
    fetch(state, id).await
}

pub async fn start(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<Pipeline>> {
    set_desired(&state, &id, DesiredState::Running).await.map(Json)
}

pub async fn stop(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<Pipeline>> {
    set_desired(&state, &id, DesiredState::Stopped).await.map(Json)
}

/// Stop, then start again. Orion has no restart verb; the engine sees the workload leave and
/// come back.
pub async fn restart(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<Pipeline>> {
    set_desired(&state, &id, DesiredState::Stopped).await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    set_desired(&state, &id, DesiredState::Running).await.map(Json)
}

pub async fn rollback(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<Pipeline>> {
    check_id(&id)?;
    let previous = state.store.pop_revision(&id).await?;
    let spec: PipelineSpec = serde_json::from_value(previous.spec).map_err(|error| ApiError::internal(format!("stored revision {} is unreadable: {error}", previous.revision)))?;
    let view = state.orion.view().await?;
    let document = validate_spec(&spec, &view, &state.config.node_id)?;
    let (artifact, workload) = records_for(&id, &spec, &document, previous.revision, &state.config.node_id)?;
    state.orion.apply(vec![DesiredStateMutation::PutArtifact(artifact), DesiredStateMutation::PutWorkload(workload)]).await?;
    fetch(&state, &id).await.map(Json)
}

#[derive(Debug, Serialize)]
pub struct RevisionSummary {
    pub revision: u64,
    pub saved_at_ms: u64,
    pub name: Option<String>,
}

pub async fn revisions(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<Vec<RevisionSummary>>> {
    check_id(&id)?;
    let history = state.store.history(&id).await?;
    Ok(Json(
        history.revisions.iter().rev().map(|r| RevisionSummary { revision: r.revision, saved_at_ms: r.saved_at_ms, name: r.spec.get("name").and_then(|n| n.as_str()).map(str::to_string) }).collect(),
    ))
}

/// Rebind one input (a new revision with the same graph).
pub async fn bind_input(State(state): State<SharedState>, Path((id, input)): Path<(String, String)>, Json(binding): Json<Binding>) -> ApiResult<Json<Pipeline>> {
    check_id(&id)?;
    let history = state.store.history(&id).await?;
    let mut spec: PipelineSpec = match history.latest() {
        Some(latest) => serde_json::from_value(latest.spec.clone()).map_err(|error| ApiError::internal(error.to_string()))?,
        None => {
            // A pipeline written by another tool: start from what Orion has.
            let pipeline = fetch(&state, &id).await?;
            PipelineSpec {
                name: pipeline.name,
                graph: pipeline.graph.ok_or_else(|| ApiError::conflict(format!("pipeline {id} has no inline graph to keep")))?,
                bindings: pipeline.bindings,
                enabled: pipeline.enabled,
            }
        }
    };
    spec.bindings.insert(input, binding);
    deploy(&state, &id, spec).await.map(Json)
}

pub async fn pipeline_outputs(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<Vec<Output>>> {
    check_id(&id)?;
    let view = state.orion.view().await?;
    let workload = find_workload(&view, &id).ok_or_else(|| ApiError::not_found(format!("no pipeline {id}")))?;
    Ok(Json(outputs_of(&view, workload.workload_id.as_str())))
}

/// The latest value of every pipeline host output.
pub async fn outputs(State(state): State<SharedState>) -> ApiResult<Json<Vec<Output>>> {
    let view = state.orion.view().await?;
    Ok(Json(engine_workloads(&view).flat_map(|w| outputs_of(&view, w.workload_id.as_str())).collect()))
}

#[derive(Debug, Serialize)]
pub struct PluginList {
    pub engine_running: bool,
    pub plugins: Vec<String>,
}

pub async fn plugins(State(state): State<SharedState>) -> ApiResult<Json<PluginList>> {
    let view = state.orion.view().await?;
    let loaded = loaded_plugins(&view, &state.config.node_id);
    Ok(Json(PluginList { engine_running: loaded.is_some(), plugins: loaded.unwrap_or_default() }))
}

pub async fn catalog() -> ApiError {
    ApiError::not_available(
        "the node catalog is not published by the device yet",
        "helios-engine publishing its plugin registry (node ids, ports with TypeExpr/TypeKey, parameters, plugin versions) as an Orion resource or artifact",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::cameras;

    pub fn sample_document() -> serde_json::Value {
        serde_json::json!({
            "format": "daedalus.graph",
            "schema_version": 1,
            "requires": [{ "id": "eidos" }],
            "metadata": { "helios.editor": { "positions": {} } },
            "graph": {
                "nodes": [
                    { "id": "helios:frames", "label": "camera", "inputs": [], "outputs": ["frame"], "metadata": { "host_bridge": { "type": "Bool", "value": true } } },
                    { "id": "eidos:aruco.decode", "label": "decode", "inputs": ["frame"], "outputs": ["detections"], "const_inputs": [["dictionary", { "type": "String", "value": "apriltag_36h11" }]] }
                ],
                "edges": [{ "from": { "node": 0, "port": "frame" }, "to": { "node": 1, "port": "frame" } }]
            }
        })
    }

    fn view_with_camera() -> StateView {
        let mut view = StateView::default();
        let camera = ResourceRecord::builder("camera.front", cameras::CAMERA_RESOURCE_TYPE, "provider.peripherals.node-local").build();
        view.resources.insert("camera.front".into(), camera);
        view
    }

    #[test]
    fn spec_round_trips_through_orion_records() {
        let view = view_with_camera();
        let spec = PipelineSpec {
            name: "Tags front".into(),
            graph: sample_document(),
            bindings: BTreeMap::from([("camera".to_string(), Binding { resource_id: "camera.front".into(), output_width: Some(640), output_height: Some(400), pyramid: Some(1), camera: None })]),
            enabled: true,
        };
        let document = validate_spec(&spec, &view, "node-local").expect("valid spec");
        let (artifact, workload) = records_for("tags-front", &spec, &document, 3, "node-local").expect("records");
        assert_eq!(workload.workload_id.as_str(), "pipeline.tags-front");
        assert_eq!(workload.runtime_type.as_str(), ENGINE_RUNTIME);
        assert_eq!(workload.resource_bindings.len(), 1);
        let mut view = view;
        view.artifacts.insert(artifact.artifact_id.to_string(), artifact);
        view.workloads.insert(workload.workload_id.to_string(), workload.clone());
        let dto = pipeline_dto(&view, &workload);
        assert_eq!(dto.id, "tags-front");
        assert_eq!(dto.name, "Tags front");
        assert_eq!(dto.revision, Some(3));
        assert_eq!(dto.bindings, spec.bindings);
        assert_eq!(dto.plugins, vec!["eidos".to_string()]);
        let graph = dto.graph.expect("graph");
        assert_eq!(graph["format"], "daedalus.graph");
        assert_eq!(graph["graph"]["nodes"].as_array().map(Vec::len), Some(2));
        assert_eq!(pipelines_using(&view, "camera.front"), vec!["tags-front".to_string()]);
    }

    #[test]
    fn bare_or_unversioned_graphs_are_rejected() {
        let view = view_with_camera();
        let mut spec = PipelineSpec { name: "x".into(), graph: serde_json::json!({ "nodes": [], "edges": [] }), bindings: BTreeMap::new(), enabled: true };
        let error = validate_spec(&spec, &view, "node-local").expect_err("bare graph JSON");
        assert_eq!(error.code, crate::error::ErrorCode::Unprocessable);
        spec.graph = sample_document();
        spec.graph["schema_version"] = serde_json::json!(99);
        assert!(validate_spec(&spec, &view, "node-local").is_err());
    }

    #[test]
    fn requires_are_checked_against_loaded_plugins() {
        let mut view = view_with_camera();
        let runtime = ResourceRecord::builder("engine.runtime.node-local", "execution.runtime", "provider.engine.node-local").label("helios.plugin.loaded=styx.frames").build();
        view.resources.insert("engine.runtime.node-local".into(), runtime);
        let spec = PipelineSpec { name: "x".into(), graph: sample_document(), bindings: BTreeMap::new(), enabled: true };
        let error = validate_spec(&spec, &view, "node-local").expect_err("eidos is not loaded");
        assert!(error.message.contains("eidos"));
    }

    #[test]
    fn bindings_must_name_known_resources() {
        let view = view_with_camera();
        let spec = PipelineSpec {
            name: "x".into(),
            graph: sample_document(),
            bindings: BTreeMap::from([("camera".into(), Binding { resource_id: "camera.nope".into(), ..Binding::default() })]),
            enabled: true,
        };
        assert!(validate_spec(&spec, &view, "node-local").is_err());
    }

    #[test]
    fn slugs_are_ids() {
        assert_eq!(slug("AprilTags · front!"), "apriltags-front");
        assert!(super::super::valid_id(&slug("")));
    }
}
