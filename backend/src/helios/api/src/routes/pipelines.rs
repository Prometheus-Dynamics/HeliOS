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
    camera_context::{self, CalibrationMatch},
    error::{ApiError, ApiResult},
    field_layouts::{self, StoredLayout},
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
/// The field layout a pipeline's multi-tag pose solves against.
const FIELD_LAYOUT_LABEL: &str = "helios.pipeline.field_layout";
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
    /// How the graph's Eidos detector groups search each frame: `tracked` (full search every
    /// `full_search_every` frames, windows around tracked tags in between) or `full` (every
    /// frame). Unset keeps what the graph has, except that a camera pipeline's untracked groups
    /// become `tracked` with `full_search_every` 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_mode: Option<SearchMode>,
    /// Tracked search only: full search at least every this many frames (1 to 100000; default
    /// 4). New tags are found at most `full_search_every - 1` frames after they appear.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_search_every: Option<u32>,
    /// The field layout (`/v1/field-layouts`) the graph's multi-tag pose solves against; unset
    /// uses the selected layout, and follows it when the selection changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_layout: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    Full,
    Tracked,
}

/// Default tracked search interval: a full search every 4th frame. On the CM5 (Eidos 2bbad728d,
/// one thread, the recorded test video, track-loss recovery and dormant tracks) it takes 0.40 to
/// 0.41 ms per frame on average (p99 about 1.0 ms) and finds 3012 of the 3149 reference tags,
/// against 3034 at 0.84 ms for full search every frame (N=8: 2988 at 0.33 ms; N=16: 2967 at
/// 0.28 to 0.29 ms). New tags are found at most 3 frames after they appear.
pub const DEFAULT_FULL_SEARCH_EVERY: u32 = 4;

/// Eidos's detector groups: (full search group, tracked group).
const DETECTOR_GROUPS: [(&str, &str); 2] = [("eidos:detectors.aruco", "eidos:detectors.aruco_tracked"), ("eidos:detectors.apriltag", "eidos:detectors.apriltag_tracked")];
/// Config ports the tracked groups have and the full ones do not (Eidos docs/daedalus.md,
/// "Tracked detector groups").
const TRACKED_ONLY_PORTS: [&str; 16] = [
    "full_search_every",
    "roi_margin",
    "roi_margin_side",
    "roi_margin_velocity",
    "max_tracks",
    "max_missed_frames",
    "margin_growth_misses",
    "prediction_horizon_frames",
    "loss_search",
    "recovery_margin_scale",
    "loss_full_search_after",
    "end_tracks_leaving_frame",
    "change_threshold",
    "change_step",
    "constant_velocity",
    "full_search",
];

/// The search mode of `graph`'s detector groups, when it has any: `(mode, full_search_every)`.
pub fn search_mode_of(graph: &serde_json::Value) -> Option<(SearchMode, Option<u32>)> {
    let nodes = graph.get("graph")?.get("nodes")?.as_array()?;
    nodes.iter().find_map(|node| {
        let id = node.get("id")?.as_str()?;
        if DETECTOR_GROUPS.iter().any(|(full, _)| *full == id) {
            return Some((SearchMode::Full, None));
        }
        if DETECTOR_GROUPS.iter().any(|(_, tracked)| *tracked == id) {
            let every = node
                .get("const_inputs")
                .and_then(|c| c.as_array())
                .and_then(|consts| consts.iter().find(|entry| entry.get(0).and_then(|n| n.as_str()) == Some("full_search_every")).and_then(|entry| entry.get(1)?.get("value")?.as_u64()));
            return Some((SearchMode::Tracked, Some(every.map_or(DEFAULT_FULL_SEARCH_EVERY, |every| every as u32))));
        }
        None
    })
}

/// Switch every Eidos detector group in `graph` to `mode`: the full and tracked groups have the
/// same ports apart from the tracking ones, so the node keeps its edges and its other constants.
/// Returns how many groups it changed or set.
pub fn apply_search_mode(graph: &mut serde_json::Value, mode: SearchMode, full_search_every: u32) -> usize {
    let Some(nodes) = graph.get_mut("graph").and_then(|g| g.get_mut("nodes")).and_then(|n| n.as_array_mut()) else {
        return 0;
    };
    let mut changed = 0;
    for node in nodes {
        let Some(id) = node.get("id").and_then(|id| id.as_str()) else { continue };
        let Some((full, tracked)) = DETECTOR_GROUPS.iter().find(|(full, tracked)| *full == id || *tracked == id) else { continue };
        changed += 1;
        match mode {
            SearchMode::Full => {
                node["id"] = (*full).into();
                if let Some(consts) = node.get_mut("const_inputs").and_then(|c| c.as_array_mut()) {
                    consts.retain(|entry| !entry.get(0).and_then(|n| n.as_str()).is_some_and(|name| TRACKED_ONLY_PORTS.contains(&name)));
                }
                if let Some(inputs) = node.get_mut("inputs").and_then(|c| c.as_array_mut()) {
                    inputs.retain(|port| !port.as_str().is_some_and(|name| TRACKED_ONLY_PORTS.contains(&name)));
                }
            }
            SearchMode::Tracked => {
                node["id"] = (*tracked).into();
                if node.get("const_inputs").and_then(|c| c.as_array()).is_none() {
                    node["const_inputs"] = serde_json::json!([]);
                }
                let consts = node["const_inputs"].as_array_mut().expect("just ensured");
                consts.retain(|entry| entry.get(0).and_then(|n| n.as_str()) != Some("full_search_every"));
                consts.push(serde_json::json!(["full_search_every", { "type": "Int", "value": full_search_every }]));
                if let Some(inputs) = node.get_mut("inputs").and_then(|c| c.as_array_mut())
                    && !inputs.iter().any(|port| port.as_str() == Some("full_search_every"))
                {
                    inputs.push("full_search_every".into());
                }
            }
        }
    }
    changed
}

/// The graph a spec deploys: its search mode applied (see `PipelineSpec::search_mode`).
pub fn effective_graph(spec: &PipelineSpec, view: &StateView) -> ApiResult<serde_json::Value> {
    let mut graph = spec.graph.clone();
    let every = spec.full_search_every.unwrap_or(DEFAULT_FULL_SEARCH_EVERY);
    if !(1..=100_000).contains(&every) {
        return Err(ApiError::unprocessable("full_search_every is 1 to 100000 frames"));
    }
    if spec.full_search_every.is_some() && spec.search_mode == Some(SearchMode::Full) {
        return Err(ApiError::unprocessable("full_search_every applies to search_mode \"tracked\" only"));
    }
    let mode = match spec.search_mode {
        Some(mode) => Some(mode),
        None if spec.full_search_every.is_some() => Some(SearchMode::Tracked),
        // Camera pipelines track by default.
        None if search_mode_of(&graph).is_some_and(|(mode, _)| mode == SearchMode::Full) && camera_inputs(spec, view).next().is_some() => Some(SearchMode::Tracked),
        None => None,
    };
    if let Some(mode) = mode {
        let changed = apply_search_mode(&mut graph, mode, every);
        if changed == 0 && spec.search_mode.is_some() {
            return Err(ApiError::unprocessable("search_mode needs an Eidos detector group in the graph (eidos:detectors.apriltag or eidos:detectors.aruco, tracked or not)"));
        }
    }
    Ok(graph)
}

/// The spec's bindings to camera resources.
fn camera_inputs<'a>(spec: &'a PipelineSpec, view: &'a StateView) -> impl Iterator<Item = (&'a String, &'a Binding)> + 'a {
    spec.bindings.iter().filter(|(_, binding)| view.resources.get(&binding.resource_id).is_some_and(|r| r.resource_type.as_str() == super::cameras::CAMERA_RESOURCE_TYPE))
}

/// The camera context of each camera binding (`camera_context`): the calibration matching its
/// frame size and the camera's mount, as `binding.<input>.context.*` fields.
pub type CameraContexts = BTreeMap<String, BTreeMap<String, TypedConfigValue>>;

/// `CameraContexts` for `bindings` (input -> binding) from the stored calibrations and mounts.
pub async fn camera_contexts<'a>(state: &SharedState, view: &StateView, bindings: impl Iterator<Item = (&'a String, &'a Binding)>) -> ApiResult<CameraContexts> {
    let calibrations = state.store.calibrations().await?;
    let mounts = state.store.mounts().await?;
    Ok(bindings
        .filter(|(_, binding)| view.resources.get(&binding.resource_id).is_some_and(|r| r.resource_type.as_str() == super::cameras::CAMERA_RESOURCE_TYPE))
        .map(|(input, binding)| {
            let size = binding.output_width.zip(binding.output_height);
            let matched = camera_context::match_calibration(calibrations.get(&binding.resource_id).map(Vec::as_slice).unwrap_or_default(), size);
            (input.clone(), camera_context::context_fields(matched.calibration(), mounts.get(&binding.resource_id)))
        })
        .collect())
}

/// Which calibration a camera binding uses, for the pipeline DTO.
pub fn calibration_note(matched: &CalibrationMatch) -> serde_json::Value {
    match matched {
        CalibrationMatch::Exact(c) => serde_json::json!({ "status": "calibrated", "width": c.width, "height": c.height, "model": c.model }),
        CalibrationMatch::Scaled { calibration: c, from } => serde_json::json!({ "status": "calibrated", "width": c.width, "height": c.height, "model": c.model, "scaled_from": [from.0, from.1] }),
        CalibrationMatch::None(reason) => serde_json::json!({ "status": "uncalibrated", "reason": reason }),
    }
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
    /// The graph's detector search: `full` or `tracked` (with `full_search_every`); `null`
    /// without an Eidos detector group.
    pub search_mode: Option<SearchMode>,
    pub full_search_every: Option<u32>,
    /// Tag and field poses from the latest outputs (`pose_solutions`, `multi_tag_pose`), with
    /// the calibration status; `null` when the graph has no pose stage.
    pub pose: Option<serde_json::Value>,
    /// The field layout the graph's multi-tag pose solves against; `null` without one.
    pub field_layout: Option<String>,
    /// Managed by this API (created through `/v1/pipelines`) rather than written by other tools.
    pub managed: bool,
}

/// The pose summary of a pipeline's outputs: Eidos's `status` (`calibrated`, `uncalibrated` or
/// `image_size_mismatch`), the tag poses, and from the multi-tag pose (`multi_tag_pose`,
/// reference = the field) the camera and robot in the field: `camera_in_field` is the camera's
/// optical frame (`reference_from_camera`), `robot_in_field` the robot frame through the camera's
/// mount (`reference_from_rig`; `null` without a mount). Both are `null` without a valid pose.
pub fn pose_summary(outputs: &[Output]) -> Option<serde_json::Value> {
    let solutions = outputs.iter().find(|o| o.port == "pose_solutions");
    let field = outputs.iter().find(|o| o.port == "multi_tag_pose");
    if solutions.is_none() && field.is_none() {
        return None;
    }
    let status = solutions.and_then(|o| o.value.get("status")).or_else(|| field.and_then(|o| o.value.get("status"))).cloned().unwrap_or(serde_json::Value::Null);
    let tags = solutions
        .and_then(|o| o.value.get("tags"))
        .and_then(|t| t.as_array())
        .map(|tags| {
            tags.iter()
                .map(|tag| serde_json::json!({ "id": tag.get("id"), "translation": tag.pointer("/best/translation"), "rotation": tag.pointer("/best/rotation"), "error_px": tag.get("best_error_px"), "ambiguity": tag.get("ambiguity") }))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let field_value = field.map(|o| &o.value);
    let valid = field_value.and_then(|f| f.get("valid")).and_then(|v| v.as_bool()).unwrap_or(false);
    let has_rig = field_value.and_then(|f| f.get("has_rig")).and_then(|v| v.as_bool()).unwrap_or(false);
    let pose = |key: &str| if valid { field_value.and_then(|f| f.get(key)).cloned() } else { None };
    Some(serde_json::json!({
        "status": status,
        "tags": tags,
        "field_valid": field_value.map(|_| valid),
        "camera_in_field": pose("reference_from_camera"),
        "robot_in_field": if has_rig { pose("reference_from_rig") } else { None },
        "field_rms_px": field_value.and_then(|f| f.get("rms_px")).cloned(),
        "field_inlier_tags": field_value.and_then(|f| f.get("inlier_tags")).cloned(),
        "field_ambiguity": field_value.and_then(|f| f.get("ambiguity")).cloned(),
        "observed_at_ms": solutions.into_iter().chain(field).map(|o| o.observed_at_ms).max(),
    }))
}

/// The pipelines whose multi-tag pose solves against layout `id`.
pub fn pipelines_using_layout(view: &StateView, id: &str) -> Vec<String> {
    engine_workloads(view)
        .filter(|w| view.artifacts.get(w.artifact_id.as_str()).is_some_and(|a| label_map(&a.labels).get(FIELD_LAYOUT_LABEL).is_some_and(|layout| layout == id)))
        .map(|w| pipeline_id(w.workload_id.as_str()))
        .collect()
}

/// Pose summary per pipeline, for `pose` events.
pub fn pose_digest(view: &StateView) -> BTreeMap<String, serde_json::Value> {
    engine_workloads(view).filter_map(|w| pose_summary(&outputs_of(view, w.workload_id.as_str())).map(|pose| (pipeline_id(w.workload_id.as_str()), pose))).collect()
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
    let (search_mode, full_search_every) = graph.as_ref().and_then(search_mode_of).map_or((None, None), |(mode, every)| (Some(mode), every));
    let outputs = outputs_of(view, &workload_id);
    let telemetry = workload_resources(view, &workload_id, ARTIFACT_TYPE)
        .find(|record| label_map(&record.labels).get("helios.artifact.kind").map(String::as_str) == Some(TELEMETRY_KIND))
        .map(|record| parse_message(state_field(record, "message")));
    Pipeline {
        id: pipeline_id(&workload_id),
        name: labels.get(NAME_LABEL).cloned().unwrap_or_else(|| pipeline_id(&workload_id)),
        revision: labels.get(REVISION_LABEL).and_then(|r| r.parse().ok()),
        field_layout: labels.get(FIELD_LAYOUT_LABEL).cloned(),
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
        pose: pose_summary(&outputs),
        outputs,
        search_mode,
        full_search_every,
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

/// Validate a spec against Daedalus's document format and the live state, and return the
/// document it deploys (its search mode applied, and `layout`'s tags in its multi-tag pose).
pub fn validate_spec(spec: &PipelineSpec, view: &StateView, node_id: &str, layout: Option<&StoredLayout>) -> ApiResult<GraphDocument> {
    if spec.name.trim().is_empty() || spec.name.len() > 120 {
        return Err(ApiError::unprocessable("name must be 1 to 120 characters"));
    }
    let mut graph = effective_graph(spec, view)?;
    if let Some(layout) = layout {
        field_layouts::apply_layout(&mut graph, &layout.layout)?;
    }
    let text = serde_json::to_string(&graph).map_err(|error| ApiError::bad_request(error.to_string()))?;
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
pub fn records_for(
    id: &str,
    spec: &PipelineSpec,
    document: &GraphDocument,
    revision: u64,
    node_id: &str,
    contexts: &CameraContexts,
    layout: Option<&str>,
) -> ApiResult<(ArtifactRecord, WorkloadRecord)> {
    let workload_id = format!("{WORKLOAD_PREFIX}{id}");
    let artifact_id = format!("{ARTIFACT_PREFIX}{id}");
    let inline = document.to_json().map_err(|error| ApiError::internal(error.to_string()))?;
    let mut artifact = ArtifactRecord::builder(ArtifactId::new(artifact_id.clone()))
        .content_type(GRAPH_CONTENT_TYPE)
        .size_bytes(inline.len() as u64)
        .label(format!("{NAME_LABEL}={}", spec.name.replace('\n', " ")))
        .label(format!("{REVISION_LABEL}={revision}"));
    if let Some(layout) = layout {
        artifact = artifact.label(format!("{FIELD_LAYOUT_LABEL}={layout}"));
    }
    let artifact = artifact.build();
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
        for (field, value) in contexts.get(input).into_iter().flatten() {
            config = config.field(format!("binding.{input}.context.{field}"), value.clone());
        }
        builder = builder.bind_resource(ResourceId::new(binding.resource_id.clone()), NodeId::new(node_id));
    }
    Ok((artifact, builder.config(config).build()))
}

/// The layout a spec's multi-tag pose gets: the one it names, else the selected one; `None` when
/// its graph has no multi-tag pose (a named layout must still exist).
async fn spec_layout(state: &SharedState, spec: &PipelineSpec) -> ApiResult<Option<StoredLayout>> {
    if let Some(named) = &spec.field_layout {
        let layout = field_layouts::layout(state, named).await?;
        return Ok(field_layouts::has_multi_tag_pose(&spec.graph).then_some(layout));
    }
    if !field_layouts::has_multi_tag_pose(&spec.graph) {
        return Ok(None);
    }
    field_layouts::layout_for(state, None).await.map(Some)
}

/// Write `spec` to Orion as revision `revision` (camera context and field layout as stored now).
async fn apply_spec(state: &SharedState, view: &StateView, id: &str, spec: &PipelineSpec, revision: u64) -> ApiResult<()> {
    let layout = spec_layout(state, spec).await?;
    let document = validate_spec(spec, view, &state.config.node_id, layout.as_ref())?;
    let contexts = camera_contexts(state, view, spec.bindings.iter()).await?;
    let (artifact, workload) = records_for(id, spec, &document, revision, &state.config.node_id, &contexts, layout.as_ref().map(|l| l.id.as_str()))?;
    state.orion.apply(vec![DesiredStateMutation::PutArtifact(artifact), DesiredStateMutation::PutWorkload(workload)]).await
}

async fn deploy(state: &SharedState, id: &str, spec: PipelineSpec) -> ApiResult<Pipeline> {
    let view = state.orion.view().await?;
    let revision = state.store.history(id).await?.next_revision();
    apply_spec(state, &view, id, &spec, revision).await?;
    let spec_json = serde_json::to_value(&spec).map_err(|error| ApiError::internal(error.to_string()))?;
    state.store.push_revision(id, spec_json, now_ms()).await?;
    fetch(state, id).await
}

/// Redeploy, at their current revision, the managed pipelines whose multi-tag pose uses a layout
/// that changed: those naming `changed` (an uploaded layout replaced), or, with `selection`, those
/// following the selected layout. Returns the pipelines updated.
pub async fn refresh_field_layouts(state: &SharedState, changed: Option<&str>, selection: bool) -> ApiResult<Vec<String>> {
    let view = state.orion.view().await?;
    let selected = field_layouts::selected_id(state).await?;
    let mut updated = Vec::new();
    for workload in engine_workloads(&view) {
        let id = pipeline_id(workload.workload_id.as_str());
        let history = state.store.history(&id).await?;
        let Some(latest) = history.latest() else { continue };
        let Ok(spec) = serde_json::from_value::<PipelineSpec>(latest.spec.clone()) else { continue };
        if !field_layouts::has_multi_tag_pose(&spec.graph) {
            continue;
        }
        let affected = match &spec.field_layout {
            Some(named) => changed == Some(named.as_str()),
            None => selection || changed == Some(selected.as_str()),
        };
        if affected {
            apply_spec(state, &view, &id, &spec, latest.revision).await?;
            updated.push(id);
        }
    }
    Ok(updated)
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
    apply_spec(&state, &view, &id, &spec, previous.revision).await?;
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
                search_mode: None,
                full_search_every: None,
                field_layout: pipeline.field_layout,
            }
        }
    };
    spec.bindings.insert(input, binding);
    deploy(&state, &id, spec).await.map(Json)
}

/// Write the current camera context (calibration, mount) of `camera` into every engine workload
/// bound to it, without a new revision: helios-engine pushes the new values into the running
/// graph (adding or removing a mount recompiles it). Returns the pipelines updated.
pub async fn refresh_camera_context(state: &SharedState, camera: &str) -> ApiResult<Vec<String>> {
    let view = state.orion.view().await?;
    let mut mutations = Vec::new();
    let mut updated = Vec::new();
    for workload in engine_workloads(&view) {
        let bindings = decode_bindings(workload);
        let using = bindings.iter().filter(|(_, b)| b.resource_id == camera).collect::<Vec<_>>();
        if using.is_empty() {
            continue;
        }
        let contexts = camera_contexts(state, &view, using.iter().map(|(input, binding)| (*input, *binding))).await?;
        let mut record = workload.clone();
        let Some(config) = record.config.as_mut() else { continue };
        for (input, _) in &using {
            let prefix = format!("binding.{input}.context.");
            config.payload.retain(|key, _| !key.starts_with(&prefix));
            for (field, value) in contexts.get(*input).into_iter().flatten() {
                config.payload.insert(format!("{prefix}{field}"), value.clone());
            }
        }
        if record.config != workload.config {
            updated.push(pipeline_id(workload.workload_id.as_str()));
            mutations.push(DesiredStateMutation::PutWorkload(record));
        }
    }
    if !mutations.is_empty() {
        state.orion.apply(mutations).await?;
    }
    Ok(updated)
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
            search_mode: None,
            full_search_every: None,
            field_layout: None,
        };
        let document = validate_spec(&spec, &view, "node-local", None).expect("valid spec");
        let contexts = BTreeMap::from([("camera".to_string(), camera_context::context_fields(None, None))]);
        let (artifact, workload) = records_for("tags-front", &spec, &document, 3, "node-local", &contexts, None).expect("records");
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
        let config = workload.config.as_ref().expect("config");
        assert_eq!(config.payload.get("binding.camera.context.camera.fx_px"), Some(&TypedConfigValue::F64(0.0)), "uncalibrated context");
        assert_eq!(config.payload.get("binding.camera.context.camera.lens"), Some(&TypedConfigValue::String("pinhole".into())));
        assert!(!config.payload.keys().any(|key| key.starts_with("binding.camera.context.mount.")), "no mount");
    }

    fn group_document(id: &str) -> serde_json::Value {
        serde_json::json!({
            "format": "daedalus.graph",
            "schema_version": 1,
            "requires": [{ "id": "eidos" }],
            "metadata": {},
            "graph": {
                "nodes": [
                    { "id": "io.host_bridge", "label": "host", "inputs": [], "outputs": ["frame"], "metadata": { "host_bridge": { "type": "Bool", "value": true } } },
                    { "id": id, "label": "detector", "inputs": ["dictionary", "frame", "full_search_every", "roi_margin"], "outputs": ["detections"],
                      "const_inputs": [["dictionary", { "type": "String", "value": "apriltag_36h11" }], ["full_search_every", { "type": "Int", "value": 4 }], ["roi_margin", { "type": "Float", "value": 16.0 }]] }
                ],
                "edges": [{ "from": { "node": 0, "port": "frame" }, "to": { "node": 1, "port": "frame" } }]
            }
        })
    }

    #[test]
    fn search_mode_switches_detector_groups() {
        let view = view_with_camera();
        let tracked = group_document("eidos:detectors.apriltag_tracked");
        assert_eq!(search_mode_of(&tracked), Some((SearchMode::Tracked, Some(4))));
        let camera = BTreeMap::from([("frame".to_string(), Binding { resource_id: "camera.front".into(), ..Binding::default() })]);
        let mut spec =
            PipelineSpec { name: "x".into(), graph: tracked.clone(), bindings: camera.clone(), enabled: true, search_mode: Some(SearchMode::Full), full_search_every: None, field_layout: None };

        let full = effective_graph(&spec, &view).expect("full");
        assert_eq!(full["graph"]["nodes"][1]["id"], "eidos:detectors.apriltag");
        assert_eq!(search_mode_of(&full), Some((SearchMode::Full, None)));
        assert_eq!(full["graph"]["nodes"][1]["const_inputs"].as_array().map(Vec::len), Some(1), "tracking constants removed: {full}");
        assert_eq!(full["graph"]["nodes"][1]["inputs"], serde_json::json!(["dictionary", "frame"]));

        spec.graph = full.clone();
        spec.search_mode = Some(SearchMode::Tracked);
        spec.full_search_every = Some(12);
        assert_eq!(search_mode_of(&effective_graph(&spec, &view).expect("tracked")), Some((SearchMode::Tracked, Some(12))));

        // Unset on a camera pipeline: untracked groups track with a full search every 4th frame.
        spec.search_mode = None;
        spec.full_search_every = None;
        assert_eq!(search_mode_of(&effective_graph(&spec, &view).expect("default")), Some((SearchMode::Tracked, Some(DEFAULT_FULL_SEARCH_EVERY))));
        // Without a camera binding the graph is kept.
        spec.bindings.clear();
        assert_eq!(search_mode_of(&effective_graph(&spec, &view).expect("kept")), Some((SearchMode::Full, None)));

        spec.search_mode = Some(SearchMode::Full);
        spec.full_search_every = Some(3);
        assert!(effective_graph(&spec, &view).is_err(), "full_search_every with full search");
        spec.graph = sample_document();
        spec.full_search_every = None;
        assert!(effective_graph(&spec, &view).is_err(), "no detector group to switch");
    }

    #[test]
    fn pose_summary_reports_status_tags_and_field() {
        let output = |port: &str, value: serde_json::Value| Output { pipeline: "p".into(), port: port.into(), value, observed_at_ms: 5 };
        assert_eq!(pose_summary(&[output("detections", serde_json::json!({}))]), None);
        let summary = pose_summary(&[
            output("pose_solutions", serde_json::json!({ "status": "calibrated", "tags": [{ "id": 1, "best": { "translation": [0.0, 0.0, 1.0], "rotation": [0.0, 0.0, 0.0, 1.0] }, "best_error_px": 0.2, "ambiguity": 0.1 }] })),
            output(
                "multi_tag_pose",
                serde_json::json!({ "status": "calibrated", "valid": true, "has_rig": true, "inlier_tags": 2, "rms_px": 0.3,
                    "reference_from_camera": { "translation": { "x": 1.0, "y": 2.0, "z": 0.5 } }, "reference_from_rig": { "translation": { "x": 0.7, "y": 2.0, "z": 0.0 } } }),
            ),
        ])
        .expect("summary");
        assert_eq!(summary["status"], "calibrated");
        assert_eq!(summary["tags"][0]["id"], 1);
        assert_eq!(summary["tags"][0]["translation"][2], 1.0);
        assert_eq!(summary["field_valid"], true);
        assert_eq!(summary["camera_in_field"]["translation"]["x"], 1.0);
        assert_eq!(summary["robot_in_field"]["translation"]["x"], 0.7);
        assert_eq!(summary["field_inlier_tags"], 2);
        // No mount (no rig) or no valid pose: no robot (and no camera) in the field.
        let no_rig = pose_summary(&[output("multi_tag_pose", serde_json::json!({ "status": "calibrated", "valid": true, "has_rig": false, "reference_from_camera": { "translation": { "x": 1.0 } }, "reference_from_rig": { "translation": { "x": 0.0 } } }))]).expect("summary");
        assert!(no_rig["robot_in_field"].is_null() && !no_rig["camera_in_field"].is_null(), "{no_rig}");
        let invalid = pose_summary(&[output("multi_tag_pose", serde_json::json!({ "status": "calibrated", "valid": false, "has_rig": true, "reference_from_camera": {}, "reference_from_rig": {} }))])
            .expect("summary");
        assert!(invalid["camera_in_field"].is_null() && invalid["robot_in_field"].is_null() && invalid["field_valid"] == false, "{invalid}");
        let uncalibrated = pose_summary(&[output("pose_solutions", serde_json::json!({ "status": "uncalibrated", "tags": [] }))]).expect("summary");
        assert_eq!(uncalibrated["status"], "uncalibrated");
    }

    #[test]
    fn bare_or_unversioned_graphs_are_rejected() {
        let view = view_with_camera();
        let mut spec = PipelineSpec {
            name: "x".into(),
            graph: serde_json::json!({ "nodes": [], "edges": [] }),
            bindings: BTreeMap::new(),
            enabled: true,
            search_mode: None,
            full_search_every: None,
            field_layout: None,
        };
        let error = validate_spec(&spec, &view, "node-local", None).expect_err("bare graph JSON");
        assert_eq!(error.code, crate::error::ErrorCode::Unprocessable);
        spec.graph = sample_document();
        spec.graph["schema_version"] = serde_json::json!(99);
        assert!(validate_spec(&spec, &view, "node-local", None).is_err());
    }

    #[test]
    fn requires_are_checked_against_loaded_plugins() {
        let mut view = view_with_camera();
        let runtime = ResourceRecord::builder("engine.runtime.node-local", "execution.runtime", "provider.engine.node-local").label("helios.plugin.loaded=styx.frames").build();
        view.resources.insert("engine.runtime.node-local".into(), runtime);
        let spec = PipelineSpec { name: "x".into(), graph: sample_document(), bindings: BTreeMap::new(), enabled: true, search_mode: None, full_search_every: None, field_layout: None };
        let error = validate_spec(&spec, &view, "node-local", None).expect_err("eidos is not loaded");
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
            search_mode: None,
            full_search_every: None,
            field_layout: None,
        };
        assert!(validate_spec(&spec, &view, "node-local", None).is_err());
    }

    #[test]
    fn slugs_are_ids() {
        assert_eq!(slug("AprilTags · front!"), "apriltags-front");
        assert!(super::super::valid_id(&slug("")));
    }
}
