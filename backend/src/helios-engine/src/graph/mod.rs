use daedalus::data::model::StructFieldValue;
use daedalus::data::model::TypeExpr as DaedalusTypeExpr;
use daedalus::data::model::Value as DaedalusValue;
use daedalus::engine::{Engine, EngineConfig, RuntimeMode};
use daedalus::gpu::{select_backend, Compute, DataCell, GpuBackendKind, GpuContextHandle, GpuOptions};
use daedalus::planner::{ComputeAffinity, Graph, GraphPatch, PatchReport};
use daedalus::runtime::executor::ExecutionTelemetry as DaedalusExecutionTelemetry;
use daedalus::runtime::executor::OwnedExecutor as DaedalusOwnedExecutor;
use daedalus::runtime::handler_registry::HandlerRegistry as DaedalusHandlers;
use daedalus::runtime::host_bridge::HOST_BRIDGE_META_KEY;
use daedalus::runtime::{
    BackpressureStrategy, EdgePolicyKind, HostBridgeManager as DaedalusBridgeManager, MetricsLevel as DaedalusMetricsLevel, RuntimePlan, RuntimeSink, RuntimeValue as DaedalusEdgePayload,
};
use image::{DynamicImage, GrayImage, Rgba, RgbaImage};
use lib_cv::modules::aruco::ArucoDetection2D;
use metrics::histogram;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::{PoisonError, TryLockError};
use std::time::Duration;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use styx::prelude::StageMetrics;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::daedalus_registry::build_daedalus_runtime_registry;
use crate::stream::{
    PipelineFlamegraphMetrics, PipelineGraphMetrics, PipelineImageWorkingSetMetrics, PipelineNodeMetrics, PipelineNodePerfMetrics, PipelineNodeRuntimeMetrics, PipelinePerfMetrics,
    PipelineSampleCacheMetrics,
};

mod builder;
pub(crate) mod context;
mod daedalus_config;
mod flamegraph;
mod multiplex;
mod payload;
mod perf;
mod policy;
mod roi;
#[cfg(test)]
mod tests;

use self::daedalus_config::{apply_daedalus_engine_config_overrides, apply_daedalus_engine_env_overrides, KEY_RUNTIME_BACKPRESSURE, KEY_RUNTIME_DEFAULT_POLICY};
use self::payload::{
    decode_runtime_value_as_aruco_detections, decode_runtime_value_fallback, graph_prefers_grayscale_input, is_aruco_detections_payload, is_image_payload, node_requires_color_input,
    preview_port_accepts_grayscale_input,
};
use self::roi::{bootstrap_auto_target_roi_rect, daedalus_value_as_bool, host_output_detection_source_port, is_roi_port, manual_roi_override_active, update_auto_target_roi_state, AutoTargetRoiState};
pub(crate) use builder::{build_graph_handle_for_manifest, build_graph_handle_for_pipeline_output};

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("failed to parse graph json: {0}")]
    Parse(String),
    #[error("graph build failed: {0}")]
    Build(String),
    #[error("graph missing host bridge")]
    MissingHostBridge,
}

#[derive(Debug, Clone, Default)]
pub struct GraphDisabledState {
    pub disabled: bool,
    pub disabled_since_ms: Option<u64>,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct GraphProcessOptions {
    pub require_image_output: bool,
    pub preview_only: bool,
}

impl Default for GraphProcessOptions {
    fn default() -> Self {
        Self { require_image_output: true, preview_only: false }
    }
}

pub enum GraphPreviewOutput {
    Image(DynamicImage),
    Gray(Arc<GrayImage>),
}

fn normalize_preview_output_for_port(output: GraphPreviewOutput, port: &str, host_output_port_types: &BTreeMap<String, DaedalusTypeExpr>) -> GraphPreviewOutput {
    if !preview_port_accepts_grayscale_input(port, host_output_port_types) {
        return output;
    }
    match output {
        GraphPreviewOutput::Image(DynamicImage::ImageLuma8(gray)) => GraphPreviewOutput::Gray(Arc::new(gray)),
        GraphPreviewOutput::Image(DynamicImage::ImageLumaA8(gray)) => GraphPreviewOutput::Gray(Arc::new(DynamicImage::ImageLumaA8(gray).to_luma8())),
        GraphPreviewOutput::Image(image) => GraphPreviewOutput::Gray(Arc::new(image.to_luma8())),
        other => other,
    }
}

impl GraphPreviewOutput {
    pub fn size_bytes(&self) -> u64 {
        match self {
            Self::Image(image) => dynamic_image_size_bytes(image),
            Self::Gray(image) => gray_image_size_bytes(image.as_ref()),
        }
    }

    pub fn into_dynamic_image(self) -> DynamicImage {
        match self {
            Self::Image(image) => image,
            Self::Gray(image) => DynamicImage::ImageLuma8(Arc::unwrap_or_clone(image)),
        }
    }
}

pub trait GraphExecutor: Send + Sync {
    /// Process an incoming frame and optionally emit a transformed frame.
    fn process(&self, image: DynamicImage) -> Option<DynamicImage>;

    /// Process a frame with explicit image-output demand.
    fn process_with_options(&self, image: DynamicImage, options: GraphProcessOptions) -> Option<DynamicImage> {
        let _ = options;
        self.process(image)
    }

    /// Process a frame for preview-only consumers without forcing grayscale outputs through
    /// `DynamicImage` and the general allocator.
    fn process_preview_with_options(&self, image: DynamicImage, options: GraphProcessOptions) -> Option<GraphPreviewOutput> {
        self.process_with_options(image, options).map(GraphPreviewOutput::Image)
    }

    /// Update per-stream calibration used by graph nodes that accept it.
    fn set_calibration(&self, _calibration: Option<crate::ipc::StreamCalibration>) {}

    /// Update pipeline input values for a running graph without rebuilding it.
    fn set_pipeline_inputs(&self, _pipeline_id: Option<uuid::Uuid>, _inputs: &BTreeMap<String, Option<DaedalusValue>>) {}

    /// Apply a graph patch to a running graph without rebuilding it.
    fn apply_graph_patch(&self, _pipeline_id: Option<uuid::Uuid>, _patch: &GraphPatch) -> Option<PatchReport> {
        None
    }

    /// Host-bridge output ports connected to the graph output bridge (graph -> host).
    fn host_output_ports(&self) -> Option<Vec<String>> {
        None
    }

    /// Hint that a host output port will be sampled soon and its last value should be retained.
    fn request_output_sample(&self, _port: &str) {}

    /// Whether a host output sample was recently requested and the graph should stay live long
    /// enough to materialize it from incoming frames.
    fn has_output_sample_demand(&self) -> bool {
        false
    }

    /// Solved types for host-bridge output ports (keyed by lowercase port name).
    fn host_output_port_types(&self) -> Option<BTreeMap<String, DaedalusTypeExpr>> {
        None
    }

    /// Latest JSON sample captured from a host-bridge output port, when available.
    fn sample_json_output(&self, _port: &str) -> Option<Value> {
        None
    }

    /// Latest typed sample captured from a host-bridge output port, when available.
    fn sample_value_output(&self, _port: &str) -> Option<DaedalusValue> {
        None
    }

    /// Latest image sample captured from a host-bridge output port, when available.
    ///
    /// Note: this is best-effort and primarily intended for host-side routing/debug use.
    fn sample_image_output(&self, _port: &str) -> Option<DynamicImage> {
        None
    }

    /// Whether this graph can currently emit a previewable image output.
    fn has_image_output(&self) -> bool {
        true
    }

    /// Whether the graph only needs grayscale source frames, allowing the runner to avoid
    /// materializing full RGB input for camera formats such as NV12.
    fn prefers_grayscale_input(&self) -> bool {
        false
    }

    fn pipeline_metrics(&self) -> Option<PipelineGraphMetrics> {
        None
    }

    fn pipeline_metrics_by_pipeline(&self) -> Option<BTreeMap<String, PipelineGraphMetrics>> {
        None
    }

    fn disabled_state(&self) -> GraphDisabledState {
        GraphDisabledState::default()
    }

    fn clear_disabled(&self) {}

    /// Enable/disable per-graph perf counters (cache misses, branch stats) collection.
    ///
    /// Note: when unavailable (e.g. feature disabled / kernel unsupported), implementations
    /// should either ignore the request or record an error that surfaces in metrics.
    fn set_perf_enabled(&self, _pipeline_id: Option<uuid::Uuid>, _enabled: bool) {}

    /// Reset rolling pipeline metrics (node timings, perf samples, last flamegraph).
    fn reset_pipeline_metrics(&self, _pipeline_id: Option<uuid::Uuid>) {}

    /// Drop idle-only retained graph state that is not needed once all image/sample demand is off.
    fn release_idle_retention(&self) {}

    /// Capture a CPU flamegraph for the running pipeline over a wall-clock duration.
    ///
    /// Implementations may reject if capture is already in progress.
    fn capture_flamegraph(&self, _pipeline_id: Option<uuid::Uuid>, _duration_ms: u64) -> Result<(), String> {
        Err("flamegraph capture unsupported".into())
    }
}

/// Receives frames on `frame` port and can expose simple metrics.
#[derive(Clone)]
pub struct HostBridgeHandle {
    metrics: StageMetrics,
    frame_tx: broadcast::Sender<Arc<DynamicImage>>,
    last_send_at: Arc<Mutex<Option<Instant>>>,
}

impl HostBridgeHandle {
    pub fn new(buffer: usize) -> (Self, broadcast::Receiver<Arc<DynamicImage>>) {
        let (tx, rx) = broadcast::channel(buffer);
        let metrics = StageMetrics::default();
        (Self { metrics, frame_tx: tx, last_send_at: Arc::new(Mutex::new(None)) }, rx)
    }

    pub fn send_frame(&self, image: Arc<DynamicImage>) {
        // Record host bridge cadence (time between frames delivered to subscribers).
        // This intentionally tracks throughput rather than the (near-zero) cost of `broadcast::send`.
        let now = Instant::now();
        if let Ok(mut last) = self.last_send_at.lock() {
            if let Some(prev) = *last {
                let mut delta = now.saturating_duration_since(prev);
                if delta.is_zero() {
                    delta = std::time::Duration::from_micros(1);
                }
                self.metrics.record(delta);
            }
            *last = Some(now);
        }
        let _ = self.frame_tx.send(image);
    }

    pub fn metrics(&self) -> StageMetrics {
        self.metrics.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<DynamicImage>> {
        self.frame_tx.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.frame_tx.receiver_count()
    }
}

#[derive(Clone)]
pub struct GraphHandle {
    pub host: HostBridgeHandle,
    executor: Option<Arc<dyn GraphExecutor>>,
    preview_executor: Option<Arc<dyn GraphExecutor>>,
}

impl std::fmt::Debug for GraphHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphHandle").field("executor_present", &self.executor.is_some()).field("preview_executor_present", &self.preview_executor.is_some()).finish()
    }
}

fn json_to_daedalus_value(value: &Value) -> DaedalusValue {
    match value {
        Value::Null => DaedalusValue::Unit,
        Value::Bool(b) => DaedalusValue::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                DaedalusValue::Int(i)
            } else if let Some(f) = n.as_f64() {
                DaedalusValue::Float(f)
            } else {
                DaedalusValue::Unit
            }
        }
        Value::String(s) => DaedalusValue::String(s.to_string().into()),
        Value::Array(items) => DaedalusValue::List(items.iter().map(json_to_daedalus_value).collect()),
        Value::Object(map) => {
            // If it already looks like a Daedalus Value (adjacently tagged), keep it as-is by
            // falling back to caller-side parsing.
            if map.contains_key("type") && map.contains_key("value") {
                return DaedalusValue::Unit;
            }
            DaedalusValue::Map(map.iter().map(|(k, v)| (DaedalusValue::String(k.to_string().into()), json_to_daedalus_value(v))).collect())
        }
    }
}

fn calibration_to_daedalus_value(calibration: Option<crate::ipc::StreamCalibration>) -> DaedalusValue {
    // The host-bridge port type for `calibration` is a Daedalus struct (`camera_calibration`).
    // Feeding it a generic map value triggers "missing calibration" coercion errors at runtime.
    // Always emit a concrete Struct payload so downstream nodes can treat it as optional based on fx/fy.
    let calib = calibration.unwrap_or(crate::ipc::StreamCalibration {
        fx: 0.0,
        fy: 0.0,
        cx: 0.0,
        cy: 0.0,
        k1: 0.0,
        k2: 0.0,
        p1: 0.0,
        p2: 0.0,
        k3: 0.0,
        undistort_iters: 5,
        lens_model: lib_cv::modules::calibration::LensModel::Pinhole,
    });

    // Emit a typed enum payload for `lensModel`. Some downstream nodes declare `lensModel` as an
    // enum in the Daedalus type system, and providing an `Int` here can cause the whole struct
    // coercion to fail (surfacing as "missing calibration" or silent passthrough in optional nodes).
    let lens_model = match calib.lens_model {
        lib_cv::modules::calibration::LensModel::Pinhole => "pinhole",
        lib_cv::modules::calibration::LensModel::Fisheye => "fisheye",
    };

    DaedalusValue::Struct(vec![
        StructFieldValue { name: "fx".into(), value: DaedalusValue::Float(calib.fx) },
        StructFieldValue { name: "fy".into(), value: DaedalusValue::Float(calib.fy) },
        StructFieldValue { name: "cx".into(), value: DaedalusValue::Float(calib.cx) },
        StructFieldValue { name: "cy".into(), value: DaedalusValue::Float(calib.cy) },
        StructFieldValue { name: "k1".into(), value: DaedalusValue::Float(calib.k1) },
        StructFieldValue { name: "k2".into(), value: DaedalusValue::Float(calib.k2) },
        StructFieldValue { name: "p1".into(), value: DaedalusValue::Float(calib.p1) },
        StructFieldValue { name: "p2".into(), value: DaedalusValue::Float(calib.p2) },
        StructFieldValue { name: "k3".into(), value: DaedalusValue::Float(calib.k3) },
        StructFieldValue { name: "lensModel".into(), value: DaedalusValue::Enum(daedalus::data::model::EnumValue { name: lens_model.into(), value: None }) },
        StructFieldValue { name: "undistortIters".into(), value: DaedalusValue::Int(calib.undistort_iters) },
    ])
}

fn default_host_bridge_input_value(port_lc: &str) -> Option<DaedalusValue> {
    match port_lc {
        // ROI control ports default to disabled crop.
        "roi_x" | "roi_y" | "roi_w" | "roi_h" => Some(DaedalusValue::Int(0)),
        // Crosshair defaults to origin unless explicitly set by controls.
        "crosshair_x" | "crosshair_y" => Some(DaedalusValue::Int(0)),
        // Overlay crosshair toggle defaults off unless explicitly enabled by controls.
        "draw_crosshair" => Some(DaedalusValue::Bool(false)),
        // Ordering defaults to no-op.
        "order_mode" => Some(DaedalusValue::String("none".into())),
        _ => None,
    }
}

fn metadata_bool_flag(map: &BTreeMap<String, DaedalusValue>, key: &str) -> Option<bool> {
    map.get(key).and_then(daedalus_value_as_bool).or_else(|| {
        map.get(key).and_then(|value| match value {
            DaedalusValue::String(raw) => {
                let raw = raw.trim().to_ascii_lowercase();
                match raw.as_str() {
                    "1" | "true" | "yes" | "on" => Some(true),
                    "0" | "false" | "no" | "off" => Some(false),
                    _ => None,
                }
            }
            _ => None,
        })
    })
}

fn normalize_graph_metadata(graph: &mut Value) {
    let Some(obj) = graph.as_object_mut() else { return };
    let Some(values) = obj.get_mut("metadata") else { return };
    let Some(values_obj) = values.as_object_mut() else { return };

    for (_, value) in values_obj.iter_mut() {
        if let Value::Object(map) = value {
            if map.contains_key("type") && map.contains_key("value") {
                continue;
            }
        }
    }
}

fn ensure_host_bridge_node_shape(node: &mut serde_json::Map<String, Value>) {
    let id = node.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let is_host_bridge = id == "io.host_bridge" || id.ends_with(":io.host_bridge");
    let is_host_output = id == "io.host_output" || id.ends_with(":io.host_output");
    if !is_host_bridge && !is_host_output {
        return;
    }

    // Ensure these nodes always carry the host-bridge marker metadata, even if the authoring UI
    // omitted it. The runtime uses this flag to derive host aliases and port bindings.
    let metadata = node.entry("metadata").or_insert_with(|| serde_json::json!({}));
    if let Some(meta_obj) = metadata.as_object_mut() {
        meta_obj.entry(HOST_BRIDGE_META_KEY.to_string()).or_insert_with(|| serde_json::to_value(DaedalusValue::Bool(true)).unwrap_or(Value::Null));
    }

    if is_host_bridge {
        // Do not mutate port lists. Host bridge ports are user-authored and can be named
        // arbitrarily; the planner will infer types from edges for dynamic ports.
        let _ = node.entry("outputs").or_insert_with(|| serde_json::json!([]));
    } else if is_host_output {
        // Host output ports are the inputs on this node.
        //
        // Do not mutate port lists here. Port shape is a graph contract and we intentionally
        // do not carry legacy shims (e.g. auto-inserting deprecated ports).
        let _ = node.entry("inputs").or_insert_with(|| serde_json::json!([]));
    }
}

fn json_string_metadata_value(value: &Value) -> Option<&str> {
    match value {
        Value::String(raw) => Some(raw.as_str()),
        Value::Object(map) => {
            let ty = map.get("type").and_then(|raw| raw.as_str())?;
            if !ty.eq_ignore_ascii_case("string") {
                return None;
            }
            map.get("value").and_then(|raw| raw.as_str())
        }
        _ => None,
    }
}

fn set_json_string_metadata_value(slot: &mut Value, raw: String) {
    match slot {
        Value::Object(map) if map.get("type").and_then(|value| value.as_str()).is_some_and(|ty| ty.eq_ignore_ascii_case("string")) => {
            map.insert("value".to_string(), Value::String(raw));
        }
        _ => *slot = Value::String(raw),
    }
}

fn prune_host_output_metadata_string_list(metadata: &mut serde_json::Map<String, Value>, key: &str, keep: &BTreeSet<String>) {
    let Some(raw) = metadata.get(key).and_then(json_string_metadata_value).map(str::to_string) else {
        return;
    };
    let filtered = raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter(|entry| {
            let port = entry.split_once(':').map(|(name, _)| name).unwrap_or(*entry).trim();
            keep.contains(&port.to_ascii_lowercase())
        })
        .map(str::to_string)
        .collect::<Vec<_>>();

    if filtered.is_empty() {
        metadata.remove(key);
        return;
    }

    if let Some(slot) = metadata.get_mut(key) {
        set_json_string_metadata_value(slot, filtered.join(","));
    }
}

fn prune_host_output_metadata_display_map(metadata: &mut serde_json::Map<String, Value>, key: &str, keep: &BTreeSet<String>) {
    let Some(raw) = metadata.get(key).and_then(json_string_metadata_value).map(str::to_string) else {
        return;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Map<String, Value>>(&raw) else {
        return;
    };
    let filtered = parsed.into_iter().filter(|(name, _)| keep.contains(&name.trim().to_ascii_lowercase())).collect::<serde_json::Map<String, Value>>();

    if filtered.is_empty() {
        metadata.remove(key);
        return;
    }

    let serialized = match serde_json::to_string(&filtered) {
        Ok(value) => value,
        Err(_) => return,
    };
    if let Some(slot) = metadata.get_mut(key) {
        set_json_string_metadata_value(slot, serialized);
    }
}

fn prune_disconnected_host_output_ports(nodes: &mut [Value], edges: &[Value]) {
    let mut connected_ports: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    let mut seen_ports: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();

    for edge in edges {
        let Some(to) = edge.get("to").and_then(|value| value.as_object()) else {
            continue;
        };
        let Some(node_idx) = to.get("node").and_then(|value| value.as_u64()).map(|value| value as usize) else {
            continue;
        };
        let Some(port) = to.get("port").and_then(|value| value.as_str()).map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        let Some(node_obj) = nodes.get(node_idx).and_then(|value| value.as_object()) else {
            continue;
        };
        let id = node_obj.get("id").and_then(|value| value.as_str()).unwrap_or("");
        if !(id == "io.host_output" || id.ends_with(":io.host_output")) {
            continue;
        }
        let key = port.to_ascii_lowercase();
        if seen_ports.entry(node_idx).or_default().insert(key) {
            connected_ports.entry(node_idx).or_default().push(port.to_string());
        }
    }

    for (idx, node) in nodes.iter_mut().enumerate() {
        let Some(node_obj) = node.as_object_mut() else { continue };
        let id = node_obj.get("id").and_then(|value| value.as_str()).unwrap_or("");
        if !(id == "io.host_output" || id.ends_with(":io.host_output")) {
            continue;
        }

        let discovered = connected_ports.remove(&idx).unwrap_or_default();
        let keep = discovered.iter().map(|port| port.to_ascii_lowercase()).collect::<BTreeSet<_>>();

        let mut pruned_inputs = Vec::new();
        let mut inserted = BTreeSet::new();
        if let Some(inputs) = node_obj.get("inputs").and_then(|value| value.as_array()) {
            for input in inputs {
                let Some(name) = input.as_str().map(str::trim).filter(|value| !value.is_empty()) else {
                    continue;
                };
                let key = name.to_ascii_lowercase();
                if keep.contains(&key) && inserted.insert(key) {
                    pruned_inputs.push(Value::String(name.to_string()));
                }
            }
        }
        for port in discovered {
            let key = port.to_ascii_lowercase();
            if inserted.insert(key) {
                pruned_inputs.push(Value::String(port));
            }
        }
        node_obj.insert("inputs".to_string(), Value::Array(pruned_inputs));

        let Some(metadata) = node_obj.get_mut("metadata").and_then(|value| value.as_object_mut()) else {
            continue;
        };
        prune_host_output_metadata_string_list(metadata, "host_bridge_inputs", &keep);
        prune_host_output_metadata_display_map(metadata, "host_bridge_inputs_display", &keep);
    }
}

fn prune_isolated_runtime_nodes(graph_obj: &mut serde_json::Map<String, Value>) {
    let Some(nodes) = graph_obj.get("nodes").and_then(|value| value.as_array()) else {
        return;
    };
    let Some(edges) = graph_obj.get("edges").and_then(|value| value.as_array()) else {
        return;
    };

    let mut degree = vec![0usize; nodes.len()];
    for edge in edges {
        let Some(edge_obj) = edge.as_object() else { continue };
        for endpoint in ["from", "to"] {
            let Some(node_idx) = edge_obj.get(endpoint).and_then(|value| value.as_object()).and_then(|value| value.get("node")).and_then(|value| value.as_u64()).map(|value| value as usize) else {
                continue;
            };
            if let Some(count) = degree.get_mut(node_idx) {
                *count += 1;
            }
        }
    }

    if degree.iter().all(|count| *count > 0) {
        return;
    }

    let mut remap: Vec<Option<usize>> = vec![None; nodes.len()];
    let mut kept_nodes = Vec::with_capacity(nodes.len());
    for (idx, node) in nodes.iter().enumerate() {
        if degree.get(idx).copied().unwrap_or_default() == 0 {
            continue;
        }
        remap[idx] = Some(kept_nodes.len());
        kept_nodes.push(node.clone());
    }

    let mut kept_edges = Vec::with_capacity(edges.len());
    for edge in edges {
        let mut edge_value = edge.clone();
        let Some(edge_obj) = edge_value.as_object_mut() else { continue };
        let mut keep_edge = true;
        for endpoint in ["from", "to"] {
            let Some(endpoint_obj) = edge_obj.get_mut(endpoint).and_then(|value| value.as_object_mut()) else {
                keep_edge = false;
                break;
            };
            let Some(old_idx) = endpoint_obj.get("node").and_then(|value| value.as_u64()).map(|value| value as usize) else {
                keep_edge = false;
                break;
            };
            let Some(new_idx) = remap.get(old_idx).and_then(|value| *value) else {
                keep_edge = false;
                break;
            };
            endpoint_obj.insert("node".to_string(), Value::from(new_idx as u64));
        }
        if keep_edge {
            kept_edges.push(edge_value);
        }
    }

    graph_obj.insert("nodes".to_string(), Value::Array(kept_nodes));
    graph_obj.insert("edges".to_string(), Value::Array(kept_edges));
}

fn normalize_graph_json_for_runtime(json: &Value) -> Value {
    let mut normalized = json.clone();
    normalize_graph_metadata(&mut normalized);

    let Some(obj) = normalized.as_object_mut() else {
        return normalized;
    };
    let edges = obj.get("edges").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    let Some(nodes) = obj.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return normalized;
    };
    for node in &mut *nodes {
        let Some(node_obj) = node.as_object_mut() else { continue };
        ensure_host_bridge_node_shape(node_obj);
    }
    prune_disconnected_host_output_ports(nodes, &edges);
    prune_isolated_runtime_nodes(obj);
    normalized
}

fn prune_unselected_host_output_edges(graph_obj: &mut serde_json::Map<String, Value>, keep_port_lc: &str) {
    let Some(nodes) = graph_obj.get("nodes").and_then(|value| value.as_array()) else {
        return;
    };
    let host_output_nodes = nodes
        .iter()
        .enumerate()
        .filter_map(|(idx, node)| {
            let node_obj = node.as_object()?;
            let id = node_obj.get("id").and_then(|value| value.as_str()).unwrap_or("");
            ((id == "io.host_output") || id.ends_with(":io.host_output")).then_some(idx)
        })
        .collect::<BTreeSet<_>>();
    if host_output_nodes.is_empty() {
        return;
    }

    let Some(edges) = graph_obj.get_mut("edges").and_then(|value| value.as_array_mut()) else {
        return;
    };
    edges.retain(|edge| {
        let Some(to) = edge.get("to").and_then(|value| value.as_object()) else {
            return true;
        };
        let Some(node_idx) = to.get("node").and_then(|value| value.as_u64()).map(|value| value as usize) else {
            return true;
        };
        if !host_output_nodes.contains(&node_idx) {
            return true;
        }
        let Some(port) = to.get("port").and_then(|value| value.as_str()) else {
            return false;
        };
        port.trim().eq_ignore_ascii_case(keep_port_lc)
    });
}

fn normalize_preview_graph_json_for_runtime(json: &Value, selected_output: &str) -> Value {
    let mut normalized = normalize_graph_json_for_runtime(json);
    let keep_port_lc = selected_output.trim().to_ascii_lowercase();
    if keep_port_lc.is_empty() {
        return normalized;
    }
    let Some(obj) = normalized.as_object_mut() else {
        return normalized;
    };
    prune_unselected_host_output_edges(obj, &keep_port_lc);
    let edges = obj.get("edges").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    if let Some(nodes) = obj.get_mut("nodes").and_then(|value| value.as_array_mut()) {
        prune_disconnected_host_output_ports(nodes, &edges);
    }
    prune_isolated_runtime_nodes(obj);
    normalized
}

fn canonicalize_graph_const_inputs(graph: &mut Graph, registry: &daedalus::runtime::plugins::PluginRegistry) {
    fn unwrap_serialized_typed_value(value: &DaedalusValue) -> Option<DaedalusValue> {
        let mut ty: Option<String> = None;
        let mut raw: Option<DaedalusValue> = None;
        match value {
            DaedalusValue::Struct(fields) => {
                for field in fields {
                    match field.name.as_str() {
                        "type" => {
                            if let DaedalusValue::String(kind) = &field.value {
                                ty = Some(kind.to_string());
                            }
                        }
                        "value" => raw = Some(field.value.clone()),
                        _ => {}
                    }
                }
            }
            _ => return None,
        }

        let kind = ty?;
        let raw = raw?;

        match kind.to_ascii_lowercase().as_str() {
            "string" | "str" => match raw {
                DaedalusValue::String(_) => Some(raw),
                _ => None,
            },
            "int" | "integer" => match raw {
                DaedalusValue::Int(_) => Some(raw),
                _ => None,
            },
            "float" | "double" => match raw {
                DaedalusValue::Float(_) => Some(raw),
                DaedalusValue::Int(i) => Some(DaedalusValue::Float(i as f64)),
                _ => None,
            },
            "bool" | "boolean" => match raw {
                DaedalusValue::Bool(_) => Some(raw),
                _ => None,
            },
            _ => Some(raw),
        }
    }

    // Canonicalize persisted graph const inputs into the runtime value form Daedalus expects.
    // Graph JSON still stores typed scalar wrappers and enum names; convert those once here.
    let view = registry.registry.view();
    for node in &mut graph.nodes {
        let Some(desc) = view.nodes.get(&daedalus::registry::ids::NodeId(node.id.0.clone())) else {
            continue;
        };
        for (port, value) in &mut node.const_inputs {
            if let Some(unwrapped) = unwrap_serialized_typed_value(value) {
                *value = unwrapped;
            }
            let Some(input) = desc.inputs.iter().find(|p| p.name == *port) else {
                continue;
            };
            let variants = match &input.ty {
                DaedalusTypeExpr::Enum(v) => v,
                DaedalusTypeExpr::Optional(inner) => {
                    if let DaedalusTypeExpr::Enum(v) = inner.as_ref() {
                        v
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };

            // Already int? leave it.
            if matches!(value, DaedalusValue::Int(_)) {
                continue;
            }

            // String -> index
            if let DaedalusValue::String(name) = value {
                if let Some((idx, _)) = variants.iter().enumerate().find(|(_, ev)| ev.name.eq_ignore_ascii_case(name.as_ref())) {
                    *value = DaedalusValue::Int(idx as i64);
                }
            }
        }
    }
}

fn canonical_fanin_input_name(name: &str, fanins: &[daedalus::registry::store::FanInPort]) -> Option<String> {
    let name_lc = name.trim().to_ascii_lowercase();
    if name_lc.is_empty() {
        return None;
    }
    for fanin in fanins {
        let prefix_raw = fanin.prefix.trim();
        if prefix_raw.is_empty() {
            continue;
        }
        let prefix_lc = prefix_raw.to_ascii_lowercase();
        let Some(suffix) = name_lc.strip_prefix(&prefix_lc) else { continue };
        let idx = if suffix.is_empty() {
            fanin.start
        } else {
            if !suffix.chars().all(|ch| ch.is_ascii_digit()) {
                continue;
            }
            let Ok(idx) = suffix.parse::<u32>() else { continue };
            if idx < fanin.start {
                continue;
            }
            idx
        };
        return Some(format!("{prefix_raw}{idx}"));
    }
    None
}

fn is_fanin_input_name(name: &str, fanins: &[daedalus::registry::store::FanInPort]) -> bool {
    canonical_fanin_input_name(name, fanins).is_some()
}

fn sync_graph_node_port_declarations(graph: &mut Graph, registry: &daedalus::runtime::plugins::PluginRegistry) {
    let view = registry.registry.view();

    // Canonicalize fan-in edge target ports so planner validation/typecheck uses the
    // same naming convention as registry `input_ty_for`.
    for edge in &mut graph.edges {
        let Some(to_node_id) = graph.nodes.get(edge.to.node.0).map(|node| node.id.clone()) else {
            continue;
        };
        let Some(desc) = view.nodes.get(&to_node_id) else {
            continue;
        };
        if let Some(canonical) = canonical_fanin_input_name(&edge.to.port, &desc.fanin_inputs) {
            edge.to.port = canonical;
        }
    }

    for node in &mut graph.nodes {
        let id = node.id.0.as_str();
        if id == "io.host_bridge" || id.ends_with(":io.host_bridge") || id == "io.host_output" || id.ends_with(":io.host_output") {
            continue;
        }

        let Some(desc) = view.nodes.get(&daedalus::registry::ids::NodeId(node.id.0.clone())) else {
            continue;
        };

        let has_dynamic_inputs = desc.metadata.contains_key("dynamic_inputs");
        let has_dynamic_outputs = desc.metadata.contains_key("dynamic_outputs");
        let fixed_inputs: Vec<String> = desc.inputs.iter().map(|p| p.name.clone()).collect();
        let fixed_outputs: Vec<String> = desc.outputs.iter().map(|p| p.name.clone()).collect();

        // Canonicalize fan-in input declarations/consts.
        for input in &mut node.inputs {
            if let Some(canonical) = canonical_fanin_input_name(input, &desc.fanin_inputs) {
                *input = canonical;
            }
        }
        for (name, _) in &mut node.const_inputs {
            if let Some(canonical) = canonical_fanin_input_name(name, &desc.fanin_inputs) {
                *name = canonical;
            }
        }

        if has_dynamic_inputs {
            let mut existing_lc: BTreeSet<String> = node.inputs.iter().map(|name| name.to_ascii_lowercase()).collect();
            for input in &fixed_inputs {
                let key = input.to_ascii_lowercase();
                if existing_lc.insert(key) {
                    node.inputs.push(input.clone());
                }
            }
        } else {
            let mut merged = fixed_inputs.clone();
            let mut merged_lc: BTreeSet<String> = merged.iter().map(|name| name.to_ascii_lowercase()).collect();
            for input in &node.inputs {
                let Some(canonical) = canonical_fanin_input_name(input, &desc.fanin_inputs) else {
                    continue;
                };
                let key = canonical.to_ascii_lowercase();
                if merged_lc.insert(key) {
                    merged.push(canonical);
                }
            }
            node.inputs = merged;
        }

        if has_dynamic_outputs {
            let mut existing_lc: BTreeSet<String> = node.outputs.iter().map(|name| name.to_ascii_lowercase()).collect();
            for output in &fixed_outputs {
                let key = output.to_ascii_lowercase();
                if existing_lc.insert(key) {
                    node.outputs.push(output.clone());
                }
            }
        } else {
            node.outputs = fixed_outputs;
        }

        if !has_dynamic_inputs {
            let fixed_input_lc: BTreeSet<String> = fixed_inputs.iter().map(|name| name.to_ascii_lowercase()).collect();
            node.const_inputs.retain(|(name, _)| fixed_input_lc.contains(&name.to_ascii_lowercase()) || is_fanin_input_name(name, &desc.fanin_inputs));
        }
    }
}

fn enforce_registry_default_compute_affinity(graph: &mut Graph, registry: &daedalus::runtime::plugins::PluginRegistry) {
    let view = registry.registry.view();
    for node in &mut graph.nodes {
        let Some(desc) = view.nodes.get(&daedalus::registry::ids::NodeId(node.id.0.clone())) else {
            continue;
        };
        // Compute affinity is defined by the node descriptor. Do not trust persisted JSON values.
        node.compute = desc.default_compute;
    }
}

fn infer_host_output_incoming_types(plan: &RuntimePlan, registry: &daedalus::registry::store::Registry) -> BTreeMap<String, BTreeMap<String, daedalus::data::model::TypeExpr>> {
    use daedalus::data::model::TypeExpr;
    use daedalus::registry::ids::NodeId;

    let mut dynamic_outputs: BTreeMap<usize, BTreeMap<String, TypeExpr>> = BTreeMap::new();
    for (idx, node) in plan.nodes.iter().enumerate() {
        let Some(DaedalusValue::Map(items)) = node.metadata.get("dynamic_output_types") else { continue };
        let mut map = BTreeMap::new();
        for (key, value) in items {
            let (DaedalusValue::String(name), DaedalusValue::String(raw)) = (key, value) else { continue };
            let Ok(ty) = serde_json::from_str::<TypeExpr>(raw) else { continue };
            map.insert(name.to_string(), ty);
        }
        if !map.is_empty() {
            dynamic_outputs.insert(idx, map);
        }
    }

    let mut out: BTreeMap<String, BTreeMap<String, TypeExpr>> = BTreeMap::new();
    let view = registry.view();
    for (from, from_port, to, to_port, _edge_policy) in &plan.edges {
        let Some(to_node) = plan.nodes.get(to.0) else { continue };
        if !(to_node.id == "io.host_output" || to_node.id.ends_with(":io.host_output")) {
            continue;
        }

        // 1) Prefer dynamic output types published by the runtime plan (e.g. host bridge / dynamic nodes).
        // 2) Fall back to the registry's node descriptor output type.
        let ty = if let Some(types) = dynamic_outputs.get(&from.0) {
            let Some(ty) = types.get(from_port) else { continue };
            ty.clone()
        } else {
            let Some(from_node) = plan.nodes.get(from.0) else { continue };
            let node_id = NodeId::new(from_node.id.clone());
            let Some(desc) = view.nodes.get(&node_id) else { continue };
            let Some(port) = desc.outputs.iter().find(|p| p.name.eq_ignore_ascii_case(from_port)) else { continue };
            port.ty.clone()
        };

        let alias = to_node.label.as_deref().unwrap_or(to_node.id.as_str()).to_ascii_lowercase();
        out.entry(alias).or_default().entry(to_port.to_ascii_lowercase()).or_insert_with(|| ty.clone());
    }

    out
}

impl GraphHandle {
    pub fn new(host: HostBridgeHandle) -> Self {
        Self { host, executor: None, preview_executor: None }
    }

    pub fn with_executor(host: HostBridgeHandle, executor: Arc<dyn GraphExecutor>) -> Self {
        Self { host, executor: Some(executor), preview_executor: None }
    }

    pub fn with_default_host(buffer: usize) -> Self {
        let (host, rx) = HostBridgeHandle::new(buffer);
        let _ = rx;
        Self { host, executor: None, preview_executor: None }
    }

    /// Build a graph handle from a Daedalus graph JSON payload. This validates the graph
    /// and installs a Daedalus-backed executor so host frames are routed through the graph.
    pub fn from_json(buffer: usize, json: &Value) -> Result<Self, GraphError> {
        Self::build_graph_handle(buffer, json, policy::pool_size(), None)
    }

    /// Build a graph handle from persisted/imported graph JSON using the same strict
    /// canonical decoding path as user-authored graphs.
    pub fn from_persisted_json(buffer: usize, json: &Value) -> Result<Self, GraphError> {
        Self::build_graph_handle(buffer, json, policy::pool_size(), None)
    }

    /// Same as `from_json` but optionally selects a single host output port to forward.
    pub fn from_json_with_output(buffer: usize, json: &Value, output_port: Option<&str>) -> Result<Self, GraphError> {
        Self::build_graph_handle(buffer, json, policy::pool_size(), output_port)
    }

    /// Same as `from_persisted_json` but optionally selects a single host output port to forward.
    pub fn from_persisted_json_with_output(buffer: usize, json: &Value, output_port: Option<&str>) -> Result<Self, GraphError> {
        Self::build_graph_handle(buffer, json, policy::pool_size(), output_port)
    }

    /// Same as `from_json` but allows overriding the Daedalus executor pool size.
    pub fn from_json_with_pool(buffer: usize, json: &Value, pool_size: Option<usize>, output_port: Option<&str>) -> Result<Self, GraphError> {
        Self::build_graph_handle(buffer, json, pool_size, output_port)
    }

    /// Same as `from_persisted_json` but allows overriding the Daedalus executor pool size.
    pub fn from_persisted_json_with_pool(buffer: usize, json: &Value, pool_size: Option<usize>, output_port: Option<&str>) -> Result<Self, GraphError> {
        Self::build_graph_handle(buffer, json, pool_size, output_port)
    }

    fn build_graph_handle(buffer: usize, json: &Value, pool_size: Option<usize>, output_port: Option<&str>) -> Result<Self, GraphError> {
        let normalized = normalize_graph_json_for_runtime(json);
        let graph: Graph = serde_json::from_value(normalized).map_err(|e| GraphError::Parse(e.to_string()))?;
        let (host, rx) = HostBridgeHandle::new(buffer);
        let _ = rx;
        let executor = DaedalusGraphExecutor::new(graph, pool_size, output_port.map(str::to_string))?;
        let preview_executor = if let Some(selected_output) = output_port {
            let preview_json = normalize_preview_graph_json_for_runtime(json, selected_output);
            let preview_graph: Graph = serde_json::from_value(preview_json).map_err(|e| GraphError::Parse(e.to_string()))?;
            let preview = DaedalusGraphExecutor::new(preview_graph, pool_size, Some(selected_output.to_string()))?;
            Some(Arc::new(preview) as Arc<dyn GraphExecutor>)
        } else {
            None
        };
        Ok(Self { host, executor: Some(Arc::new(executor)), preview_executor })
    }

    pub fn process(&self, image: DynamicImage) -> Option<DynamicImage> {
        self.process_with_options(image, GraphProcessOptions::default())
    }

    pub fn process_with_options(&self, image: DynamicImage, options: GraphProcessOptions) -> Option<DynamicImage> {
        if options.preview_only && !self.has_output_sample_demand() {
            if let Some(preview_exec) = &self.preview_executor {
                return preview_exec.process_preview_with_options(image, options).map(GraphPreviewOutput::into_dynamic_image);
            }
        }
        if let Some(exec) = &self.executor {
            exec.process_with_options(image, options)
        } else if options.require_image_output {
            Some(image)
        } else {
            None
        }
    }

    pub fn process_preview_with_options(&self, image: DynamicImage, options: GraphProcessOptions) -> Option<GraphPreviewOutput> {
        if options.preview_only && !self.has_output_sample_demand() {
            if let Some(preview_exec) = &self.preview_executor {
                return preview_exec.process_preview_with_options(image, options);
            }
        }
        if let Some(exec) = &self.executor {
            exec.process_preview_with_options(image, options)
        } else if options.require_image_output {
            Some(GraphPreviewOutput::Image(image))
        } else {
            None
        }
    }

    pub fn set_calibration(&self, calibration: Option<crate::ipc::StreamCalibration>) {
        let should_clear = calibration.is_some();
        if let Some(exec) = &self.executor {
            exec.set_calibration(calibration.clone());
            if should_clear {
                exec.clear_disabled();
            }
        }
        if let Some(exec) = &self.preview_executor {
            exec.set_calibration(calibration);
            if should_clear {
                exec.clear_disabled();
            }
        }
    }

    pub fn set_pipeline_inputs(&self, pipeline_id: Option<uuid::Uuid>, inputs: &BTreeMap<String, Option<Value>>) {
        if inputs.is_empty() {
            return;
        }
        let mut mapped: BTreeMap<String, Option<DaedalusValue>> = BTreeMap::new();
        for (port, value) in inputs {
            let key = port.trim().to_ascii_lowercase();
            if key.is_empty() {
                continue;
            }
            mapped.insert(key, value.as_ref().map(json_to_daedalus_value));
        }
        self.set_pipeline_input_values(pipeline_id, &mapped);
    }

    pub fn apply_graph_patch(&self, pipeline_id: Option<uuid::Uuid>, patch: &GraphPatch) -> Option<PatchReport> {
        let exec = self.executor.as_ref()?;
        let report = exec.apply_graph_patch(pipeline_id, patch);
        if let Some(preview_exec) = &self.preview_executor {
            let _ = preview_exec.apply_graph_patch(pipeline_id, patch);
        }
        report
    }

    pub fn set_pipeline_input_values(&self, pipeline_id: Option<uuid::Uuid>, inputs: &BTreeMap<String, Option<DaedalusValue>>) {
        if let Some(exec) = &self.executor {
            exec.set_pipeline_inputs(pipeline_id, inputs);
        }
        if let Some(exec) = &self.preview_executor {
            exec.set_pipeline_inputs(pipeline_id, inputs);
        }
    }

    pub fn host(&self) -> HostBridgeHandle {
        self.host.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<DynamicImage>> {
        self.host.subscribe()
    }

    pub fn has_executor(&self) -> bool {
        self.executor.is_some()
    }

    pub fn has_image_output(&self) -> bool {
        self.executor.as_ref().map(|exec| exec.has_image_output()).unwrap_or(true)
    }

    pub fn prefers_grayscale_input(&self) -> bool {
        self.executor.as_ref().map(|exec| exec.prefers_grayscale_input()).unwrap_or(false)
    }

    pub fn has_output_sample_demand(&self) -> bool {
        self.executor.as_ref().map(|exec| exec.has_output_sample_demand()).unwrap_or(false)
    }

    pub fn pipeline_metrics(&self) -> Option<PipelineGraphMetrics> {
        if !self.has_output_sample_demand() {
            if let Some(preview_exec) = &self.preview_executor {
                return preview_exec.pipeline_metrics().or_else(|| self.executor.as_ref().and_then(|exec| exec.pipeline_metrics()));
            }
        }
        self.executor.as_ref().and_then(|exec| exec.pipeline_metrics())
    }

    pub fn pipeline_metrics_by_pipeline(&self) -> Option<BTreeMap<String, PipelineGraphMetrics>> {
        if !self.has_output_sample_demand() {
            if let Some(preview_exec) = &self.preview_executor {
                return preview_exec.pipeline_metrics_by_pipeline().or_else(|| self.executor.as_ref().and_then(|exec| exec.pipeline_metrics_by_pipeline()));
            }
        }
        self.executor.as_ref().and_then(|exec| exec.pipeline_metrics_by_pipeline())
    }

    pub fn host_output_ports(&self) -> Option<Vec<String>> {
        self.executor.as_ref().and_then(|exec| exec.host_output_ports())
    }

    pub fn request_output_sample(&self, port: &str) {
        if let Some(exec) = &self.executor {
            exec.request_output_sample(port);
        }
    }

    pub fn host_output_port_types(&self) -> Option<BTreeMap<String, DaedalusTypeExpr>> {
        self.executor.as_ref().and_then(|exec| exec.host_output_port_types())
    }

    pub fn host_output_port_descriptors(&self) -> Vec<crate::ipc::GraphOutputPortDescriptor> {
        let Some(ports) = self.host_output_ports() else {
            return Vec::new();
        };
        let types = self.host_output_port_types().unwrap_or_default();
        ports
            .into_iter()
            .map(|name| {
                let key = name.to_ascii_lowercase();
                let ty = types.get(&key).cloned();
                let previewable = ty.as_ref().map(is_image_payload).unwrap_or(false);
                let ty = ty.and_then(|expr| serde_json::to_value(expr).ok()).map(crate::ipc::JsonWire);
                crate::ipc::GraphOutputPortDescriptor { name, ty, previewable }
            })
            .collect()
    }

    pub fn read_json_output(&self, port: &str, fresh: bool) -> Option<Value> {
        if fresh {
            self.request_output_sample(port);
        }
        self.executor.as_ref().and_then(|exec| exec.sample_json_output(port))
    }

    pub fn sample_json_output(&self, port: &str) -> Option<Value> {
        self.read_json_output(port, true)
    }

    pub fn sample_value_output(&self, port: &str) -> Option<DaedalusValue> {
        self.request_output_sample(port);
        self.executor.as_ref().and_then(|exec| exec.sample_value_output(port))
    }

    pub fn sample_image_output(&self, port: &str) -> Option<DynamicImage> {
        self.request_output_sample(port);
        self.executor.as_ref().and_then(|exec| exec.sample_image_output(port))
    }

    pub fn disabled_state(&self) -> GraphDisabledState {
        if !self.has_output_sample_demand() {
            if let Some(preview_exec) = &self.preview_executor {
                return preview_exec.disabled_state();
            }
        }
        self.executor.as_ref().map(|exec| exec.disabled_state()).unwrap_or_default()
    }

    pub fn clear_disabled(&self) {
        if let Some(exec) = &self.executor {
            exec.clear_disabled();
        }
        if let Some(exec) = &self.preview_executor {
            exec.clear_disabled();
        }
    }

    pub fn set_perf_enabled(&self, pipeline_id: Option<uuid::Uuid>, enabled: bool) {
        if let Some(exec) = &self.executor {
            exec.set_perf_enabled(pipeline_id, enabled);
        }
        if let Some(exec) = &self.preview_executor {
            exec.set_perf_enabled(pipeline_id, enabled);
        }
    }

    pub fn reset_pipeline_metrics(&self, pipeline_id: Option<uuid::Uuid>) {
        if let Some(exec) = &self.executor {
            exec.reset_pipeline_metrics(pipeline_id);
        }
        if let Some(exec) = &self.preview_executor {
            exec.reset_pipeline_metrics(pipeline_id);
        }
    }

    pub fn release_idle_retention(&self) {
        if let Some(exec) = &self.executor {
            exec.release_idle_retention();
        }
        if let Some(exec) = &self.preview_executor {
            exec.release_idle_retention();
        }
    }

    pub fn capture_flamegraph(&self, pipeline_id: Option<uuid::Uuid>, duration_ms: u64) -> Result<(), String> {
        let exec = self.executor.as_ref().ok_or_else(|| "graph has no executor".to_string())?;
        exec.capture_flamegraph(pipeline_id, duration_ms)
    }
}

/// Daedalus-backed executor that validates, plans, and executes a graph using the host bridge.
struct DaedalusGraphExecutor {
    plan: Arc<RuntimePlan>,
    handlers: DaedalusHandlers,
    pool_size: Option<usize>,
    const_coercers: daedalus::runtime::io::ConstCoercerMap,
    output_movers: daedalus::runtime::io::OutputMoverMap,
    dedicated_executor: bool,
    busy_behavior: ExecutorBusyBehavior,
    busy_timeout: Option<Duration>,
    host_mgr: DaedalusBridgeManager,
    gpu: Option<GpuContextHandle>,
    input_host_alias: String,
    input_port: String,
    calibration_port: Option<String>,
    declared_input_ports_lc: BTreeSet<String>,
    output_hosts: Vec<String>,
    host_output_ports: Vec<String>,
    host_output_ports_lc: BTreeSet<String>,
    /// Solved types for declared host output ports (keyed by lowercase port name).
    host_output_port_types: BTreeMap<String, DaedalusTypeExpr>,
    host_output_port_owners: BTreeMap<String, usize>,
    preview_ports: Vec<String>,
    preview_ports_lc: BTreeSet<String>,
    preview_aux_ports: Vec<String>,
    preview_aux_ports_lc: BTreeSet<String>,
    prefers_grayscale_input: bool,
    run_mode: RuntimeMode,
    run_metrics_level: DaedalusMetricsLevel,
    active_nodes_with_image: Option<Arc<Vec<bool>>>,
    active_nodes_preview_only: Option<Arc<Vec<bool>>>,
    active_nodes_without_image: Option<Arc<Vec<bool>>>,
    executor: Arc<std::sync::Mutex<DaedalusOwnedExecutor<DaedalusHandlers>>>,
    metrics: Mutex<RollingGraphMetrics>,
    value_samples: Mutex<BTreeMap<String, DaedalusValue>>,
    typed_samples: Mutex<BTreeMap<String, TypedHostOutputSample>>,
    image_samples: Mutex<BTreeMap<String, DynamicImage>>,
    requested_sample_ports: Mutex<BTreeMap<String, u64>>,
    image_working_set: GraphImageWorkingSetTracker,
    process_calls: AtomicU64,
    perf_enabled: AtomicBool,
    pprof_pending: AtomicBool,
    pprof_remaining: AtomicU64,
    /// Wall-clock deadline for flamegraph capture (ms since epoch). 0 disables.
    pprof_until_ms: AtomicU64,
    pprof_guard: Mutex<Option<flamegraph::FlamegraphGuard>>,
    calibration_payload: std::sync::RwLock<DaedalusValue>,
    default_input_values: BTreeMap<String, DaedalusValue>,
    input_values: std::sync::RwLock<BTreeMap<String, DaedalusValue>>,
    auto_target_roi_source_port: Option<String>,
    auto_target_roi: Mutex<AutoTargetRoiState>,
    last_background_trim_ms: AtomicU64,
    last_error_detail: std::sync::RwLock<String>,
    failure_count: AtomicU64,
    disabled: AtomicBool,
    disabled_since_ms: AtomicU64,
    rebuild_requested: AtomicBool,
}

#[derive(Debug, Default)]
struct GraphImageWorkingSetTracker {
    input_image_bytes: AtomicU64,
    host_output_image_bytes: AtomicU64,
    preview_image_bytes: AtomicU64,
    total_materialized_image_bytes: AtomicU64,
    peak_total_materialized_image_bytes: AtomicU64,
}

#[derive(Debug, Clone)]
enum TypedHostOutputSample {
    ArucoDetections(Arc<Vec<ArucoDetection2D>>),
}

impl TypedHostOutputSample {
    fn to_json(&self) -> Option<Value> {
        match self {
            Self::ArucoDetections(detections) => serde_json::to_value(detections.as_ref()).ok(),
        }
    }

    fn to_daedalus_value(&self) -> Option<DaedalusValue> {
        self.to_json().map(|json| json_to_daedalus_value(&json))
    }

    fn size_bytes(&self) -> u64 {
        match self {
            Self::ArucoDetections(detections) => serde_json::to_vec(detections.as_ref()).map(|bytes| bytes.len() as u64).unwrap_or(0),
        }
    }
}

impl GraphImageWorkingSetTracker {
    fn update_peak(slot: &AtomicU64, value: u64) {
        let mut current = slot.load(Ordering::Relaxed);
        while value > current {
            match slot.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }

    fn record(&self, input: u64, host_output: u64, preview: u64) {
        let total = input.saturating_add(host_output).saturating_add(preview);
        self.input_image_bytes.store(input, Ordering::Relaxed);
        self.host_output_image_bytes.store(host_output, Ordering::Relaxed);
        self.preview_image_bytes.store(preview, Ordering::Relaxed);
        self.total_materialized_image_bytes.store(total, Ordering::Relaxed);
        Self::update_peak(&self.peak_total_materialized_image_bytes, total);
    }

    fn clear_current(&self) {
        self.input_image_bytes.store(0, Ordering::Relaxed);
        self.host_output_image_bytes.store(0, Ordering::Relaxed);
        self.preview_image_bytes.store(0, Ordering::Relaxed);
        self.total_materialized_image_bytes.store(0, Ordering::Relaxed);
    }

    fn snapshot(&self) -> Option<PipelineImageWorkingSetMetrics> {
        let input = self.input_image_bytes.load(Ordering::Relaxed);
        let host_output = self.host_output_image_bytes.load(Ordering::Relaxed);
        let preview = self.preview_image_bytes.load(Ordering::Relaxed);
        let total = self.total_materialized_image_bytes.load(Ordering::Relaxed);
        let peak = self.peak_total_materialized_image_bytes.load(Ordering::Relaxed);
        if input == 0 && host_output == 0 && preview == 0 && total == 0 && peak == 0 {
            None
        } else {
            Some(PipelineImageWorkingSetMetrics {
                input_image_bytes: input,
                host_output_image_bytes: host_output,
                preview_image_bytes: preview,
                total_materialized_image_bytes: total,
                peak_total_materialized_image_bytes: peak,
            })
        }
    }
}

const NODE_METRICS_WINDOW_OFF: usize = 16;
const NODE_METRICS_WINDOW_BASIC: usize = 32;
const NODE_METRICS_WINDOW_DETAILED: usize = 64;
const NODE_METRICS_WINDOW_PROFILE: usize = 100;

fn node_metrics_window(level: DaedalusMetricsLevel) -> usize {
    match level {
        DaedalusMetricsLevel::Off => NODE_METRICS_WINDOW_OFF,
        DaedalusMetricsLevel::Basic => NODE_METRICS_WINDOW_BASIC,
        DaedalusMetricsLevel::Detailed => NODE_METRICS_WINDOW_DETAILED,
        DaedalusMetricsLevel::Profile => NODE_METRICS_WINDOW_PROFILE,
    }
}
const GRAPH_ERROR_DISABLE_THRESHOLD: u64 = 5;

#[derive(Debug, Clone)]
struct NodeInfo {
    type_id: String,
    label: Option<String>,
    group: Option<String>,
}

#[derive(Debug, Clone)]
struct EdgeInfo {
    from_node_index: usize,
    from_node_label: Option<String>,
    from_port: String,
    to_node_index: usize,
    to_node_label: Option<String>,
    to_port: String,
    queue_capacity: Option<u64>,
    policy: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct NodePerfSample {
    cache_misses: f64,
    branch_instructions: f64,
    branch_misses: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct NodePayloadSample {
    average_input_payload_bytes: f64,
    average_output_payload_bytes: f64,
    peak_input_payload_bytes: u64,
    peak_output_payload_bytes: u64,
    peak_payload_working_set_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct EdgeMetricSample {
    total_wait: Duration,
    samples: usize,
    max_depth: u64,
    current_depth: u64,
    peak_queue_bytes: u64,
    current_queue_bytes: u64,
    capacity: Option<u64>,
    drops: u64,
    transport_bytes: u64,
    transport_count: u64,
    gpu_uploads: u64,
    gpu_downloads: u64,
}

#[derive(Debug, Default)]
struct RollingGraphMetrics {
    window: usize,
    node_info: Vec<NodeInfo>,
    edge_info: Vec<EdgeInfo>,
    samples: BTreeMap<usize, VecDeque<(Instant, f64)>>,
    node_perf_samples: BTreeMap<usize, VecDeque<NodePerfSample>>,
    node_perf_last_at: BTreeMap<usize, Instant>,
    node_payload_samples: BTreeMap<usize, VecDeque<NodePayloadSample>>,
    node_payload_last_at: BTreeMap<usize, Instant>,
    edge_samples: BTreeMap<usize, VecDeque<EdgeMetricSample>>,
    edge_last_at: BTreeMap<usize, Instant>,
    group_samples: BTreeMap<String, VecDeque<(Instant, f64)>>,
    group_perf_samples: BTreeMap<String, VecDeque<NodePerfSample>>,
    group_perf_last_at: BTreeMap<String, Instant>,
    group_payload_samples: BTreeMap<String, VecDeque<NodePayloadSample>>,
    group_payload_last_at: BTreeMap<String, Instant>,
    graph_samples: VecDeque<(Instant, f64)>,
    wrapper_samples: VecDeque<f64>,
    wrapper_last_at: Option<Instant>,
    lock_samples: VecDeque<f64>,
    lock_last_at: Option<Instant>,
    output_materialization_samples: VecDeque<f64>,
    output_materialization_last_at: Option<Instant>,
    perf_samples: VecDeque<perf::PerfSample>,
    perf_last_at: Option<Instant>,
    last_flamegraph: Option<flamegraph::FlamegraphCapture>,
    warnings: VecDeque<(Instant, String)>,
    last_errors: BTreeMap<String, (Instant, String)>,
}

impl RollingGraphMetrics {
    fn new(window: usize, node_info: Vec<NodeInfo>, edge_info: Vec<EdgeInfo>) -> Self {
        Self {
            window,
            node_info,
            edge_info,
            samples: BTreeMap::new(),
            node_perf_samples: BTreeMap::new(),
            node_perf_last_at: BTreeMap::new(),
            node_payload_samples: BTreeMap::new(),
            node_payload_last_at: BTreeMap::new(),
            edge_samples: BTreeMap::new(),
            edge_last_at: BTreeMap::new(),
            group_samples: BTreeMap::new(),
            group_perf_samples: BTreeMap::new(),
            group_perf_last_at: BTreeMap::new(),
            group_payload_samples: BTreeMap::new(),
            group_payload_last_at: BTreeMap::new(),
            graph_samples: VecDeque::new(),
            wrapper_samples: VecDeque::new(),
            wrapper_last_at: None,
            lock_samples: VecDeque::new(),
            lock_last_at: None,
            output_materialization_samples: VecDeque::new(),
            output_materialization_last_at: None,
            perf_samples: VecDeque::new(),
            perf_last_at: None,
            last_flamegraph: None,
            warnings: VecDeque::new(),
            last_errors: BTreeMap::new(),
        }
    }

    fn record_telemetry(&mut self, telemetry: &DaedalusExecutionTelemetry) {
        let now = Instant::now();
        for warning in telemetry.warnings.iter().cloned() {
            if is_internal_runtime_warning(&warning) {
                continue;
            }
            self.record_warning(warning);
        }
        // Surface runtime node failures in the same warning/error surface the UI already shows.
        // These used to be invisible unless the whole graph run failed.
        for failure in telemetry.errors.iter() {
            let node_type = failure.node_id.as_str();
            let msg = format!("{}: {}", failure.code, failure.message);
            let prev = self.last_errors.get(node_type).map(|(_, m)| m.as_str()).unwrap_or("");
            if prev != msg {
                self.last_errors.insert(node_type.to_string(), (now, msg.clone()));
                // Keep a human-visible entry in the rolling warnings list too.
                self.warnings.push_back((now, format!("[error] {node_type}: {msg}")));
                while self.warnings.len() > self.window {
                    self.warnings.pop_front();
                }
            }
        }
        for (node_idx, node_metrics) in &telemetry.node_metrics {
            let calls = node_metrics.calls.max(1) as f64;
            let avg_ms = node_metrics.total_duration.as_secs_f64() * 1000.0 / calls;
            let deque = self.samples.entry(*node_idx).or_default();
            deque.push_back((now, avg_ms));
            while deque.len() > self.window {
                deque.pop_front();
            }
            if let Some(perf) = node_metrics.perf.as_ref() {
                let sample =
                    NodePerfSample { cache_misses: perf.cache_misses as f64 / calls, branch_instructions: perf.branch_instructions as f64 / calls, branch_misses: perf.branch_misses as f64 / calls };
                let perf_deque = self.node_perf_samples.entry(*node_idx).or_default();
                perf_deque.push_back(sample);
                while perf_deque.len() > self.window {
                    perf_deque.pop_front();
                }
                self.node_perf_last_at.insert(*node_idx, now);
            }
            if let Some(payload) = node_metrics.transport.as_ref() {
                let sample = NodePayloadSample {
                    average_input_payload_bytes: payload.in_bytes as f64 / calls,
                    average_output_payload_bytes: payload.out_bytes as f64 / calls,
                    peak_input_payload_bytes: payload.peak_input_bytes,
                    peak_output_payload_bytes: payload.peak_output_bytes,
                    peak_payload_working_set_bytes: payload.peak_working_set_bytes,
                };
                let payload_deque = self.node_payload_samples.entry(*node_idx).or_default();
                payload_deque.push_back(sample);
                while payload_deque.len() > self.window {
                    payload_deque.pop_front();
                }
                self.node_payload_last_at.insert(*node_idx, now);
            }
        }
        for (group_id, group_metrics) in &telemetry.group_metrics {
            let total_ms = group_metrics.total_duration.as_secs_f64() * 1000.0;
            let deque = self.group_samples.entry(group_id.clone()).or_default();
            deque.push_back((now, total_ms));
            while deque.len() > self.window {
                deque.pop_front();
            }
            if let Some(perf) = group_metrics.perf.as_ref() {
                let sample = NodePerfSample { cache_misses: perf.cache_misses as f64, branch_instructions: perf.branch_instructions as f64, branch_misses: perf.branch_misses as f64 };
                let perf_deque = self.group_perf_samples.entry(group_id.clone()).or_default();
                perf_deque.push_back(sample);
                while perf_deque.len() > self.window {
                    perf_deque.pop_front();
                }
                self.group_perf_last_at.insert(group_id.clone(), now);
            }
            if let Some(payload) = group_metrics.transport.as_ref() {
                let calls = group_metrics.calls.max(1) as f64;
                let sample = NodePayloadSample {
                    average_input_payload_bytes: payload.in_bytes as f64 / calls,
                    average_output_payload_bytes: payload.out_bytes as f64 / calls,
                    peak_input_payload_bytes: payload.peak_input_bytes,
                    peak_output_payload_bytes: payload.peak_output_bytes,
                    peak_payload_working_set_bytes: payload.peak_working_set_bytes,
                };
                let payload_deque = self.group_payload_samples.entry(group_id.clone()).or_default();
                payload_deque.push_back(sample);
                while payload_deque.len() > self.window {
                    payload_deque.pop_front();
                }
                self.group_payload_last_at.insert(group_id.clone(), now);
            }
        }
        for (edge_idx, edge_metrics) in &telemetry.edge_metrics {
            let deque = self.edge_samples.entry(*edge_idx).or_default();
            deque.push_back(EdgeMetricSample {
                total_wait: edge_metrics.total_wait,
                samples: edge_metrics.samples,
                max_depth: edge_metrics.max_depth,
                current_depth: edge_metrics.current_depth,
                peak_queue_bytes: edge_metrics.peak_queue_bytes,
                current_queue_bytes: edge_metrics.current_queue_bytes,
                capacity: edge_metrics.capacity,
                drops: edge_metrics.drops,
                transport_bytes: edge_metrics.transport_bytes,
                transport_count: edge_metrics.transport_count,
                gpu_uploads: edge_metrics.gpu_uploads,
                gpu_downloads: edge_metrics.gpu_downloads,
            });
            while deque.len() > self.window {
                deque.pop_front();
            }
            self.edge_last_at.insert(*edge_idx, now);
        }
    }

    fn record_graph_duration(&mut self, duration: Duration) {
        let now = Instant::now();
        let ms = duration.as_secs_f64() * 1000.0;
        self.graph_samples.push_back((now, ms));
        while self.graph_samples.len() > self.window {
            self.graph_samples.pop_front();
        }
    }

    fn record_wrapper_duration(&mut self, duration: Duration) {
        let now = Instant::now();
        let ms = duration.as_secs_f64() * 1000.0;
        self.wrapper_samples.push_back(ms);
        while self.wrapper_samples.len() > self.window {
            self.wrapper_samples.pop_front();
        }
        self.wrapper_last_at = Some(now);
    }

    fn record_lock_duration(&mut self, duration: Duration) {
        let now = Instant::now();
        let ms = duration.as_secs_f64() * 1000.0;
        self.lock_samples.push_back(ms);
        while self.lock_samples.len() > self.window {
            self.lock_samples.pop_front();
        }
        self.lock_last_at = Some(now);
    }

    fn record_output_materialization_duration(&mut self, duration: Duration) {
        let now = Instant::now();
        let ms = duration.as_secs_f64() * 1000.0;
        self.output_materialization_samples.push_back(ms);
        while self.output_materialization_samples.len() > self.window {
            self.output_materialization_samples.pop_front();
        }
        self.output_materialization_last_at = Some(now);
    }

    fn record_perf_sample(&mut self, sample: perf::PerfSample) {
        let now = Instant::now();
        self.perf_samples.push_back(sample);
        while self.perf_samples.len() > self.window {
            self.perf_samples.pop_front();
        }
        self.perf_last_at = Some(now);
    }

    fn record_flamegraph(&mut self, capture: flamegraph::FlamegraphCapture) {
        self.last_flamegraph = Some(capture);
    }

    fn record_error(&mut self, node_type: Option<&str>, error: String) {
        let now = Instant::now();
        if let Some(node_type) = node_type {
            self.last_errors.insert(node_type.to_string(), (now, error));
        } else {
            self.record_warning(error);
        }
    }

    fn record_warning(&mut self, warning: String) {
        let now = Instant::now();
        self.warnings.push_back((now, warning));
        while self.warnings.len() > self.window {
            self.warnings.pop_front();
        }
    }

    fn reset(&mut self) {
        self.samples.clear();
        self.node_perf_samples.clear();
        self.node_perf_last_at.clear();
        self.node_payload_samples.clear();
        self.node_payload_last_at.clear();
        self.edge_samples.clear();
        self.edge_last_at.clear();
        self.group_samples.clear();
        self.group_perf_samples.clear();
        self.group_perf_last_at.clear();
        self.group_payload_samples.clear();
        self.group_payload_last_at.clear();
        self.graph_samples.clear();
        self.wrapper_samples.clear();
        self.wrapper_last_at = None;
        self.lock_samples.clear();
        self.lock_last_at = None;
        self.output_materialization_samples.clear();
        self.output_materialization_last_at = None;
        self.perf_samples.clear();
        self.perf_last_at = None;
        self.last_flamegraph = None;
        self.warnings.clear();
        self.last_errors.clear();
    }

    fn release_idle_retention(&mut self) {
        self.samples.clear();
        self.node_perf_samples.clear();
        self.node_perf_last_at.clear();
        self.node_payload_samples.clear();
        self.node_payload_last_at.clear();
        self.edge_samples.clear();
        self.edge_last_at.clear();
        self.group_samples.clear();
        self.group_perf_samples.clear();
        self.group_perf_last_at.clear();
        self.group_payload_samples.clear();
        self.group_payload_last_at.clear();
        self.graph_samples.clear();
        self.wrapper_samples.clear();
        self.wrapper_last_at = None;
        self.lock_samples.clear();
        self.lock_last_at = None;
        self.output_materialization_samples.clear();
        self.output_materialization_last_at = None;
        self.perf_samples.clear();
        self.perf_last_at = None;
        self.last_flamegraph = None;
    }

    fn snapshot(&self) -> PipelineGraphMetrics {
        let now = Instant::now();
        let summarize_payload = |payload_deque: &VecDeque<NodePayloadSample>| {
            let sample_count = payload_deque.len() as f64;
            let average_input_payload_bytes = payload_deque.iter().map(|sample| sample.average_input_payload_bytes).sum::<f64>() / sample_count.max(1.0);
            let average_output_payload_bytes = payload_deque.iter().map(|sample| sample.average_output_payload_bytes).sum::<f64>() / sample_count.max(1.0);
            let peak_input_payload_bytes = payload_deque.iter().map(|sample| sample.peak_input_payload_bytes).max().unwrap_or(0);
            let peak_output_payload_bytes = payload_deque.iter().map(|sample| sample.peak_output_payload_bytes).max().unwrap_or(0);
            let peak_payload_working_set_bytes = payload_deque.iter().map(|sample| sample.peak_payload_working_set_bytes).max().unwrap_or(0);
            (average_input_payload_bytes, average_output_payload_bytes, peak_input_payload_bytes, peak_output_payload_bytes, peak_payload_working_set_bytes)
        };
        let summarize_timing = |deque: &VecDeque<f64>, last_at: Option<Instant>| -> Option<crate::stream::PipelineTimingMetrics> {
            if deque.is_empty() {
                return None;
            }
            let sample_count = deque.len() as u64;
            let sum_ms: f64 = deque.iter().copied().sum();
            let average_time_ms = sum_ms / sample_count.max(1) as f64;
            let last_time_ms = deque.back().copied().unwrap_or(0.0);
            Some(crate::stream::PipelineTimingMetrics {
                average_time_ms,
                last_time_ms,
                sample_count,
                window_size: self.window as u64,
                last_sample_age_ms: last_at.map(|instant| now.saturating_duration_since(instant).as_millis() as u64),
            })
        };
        let mut out = BTreeMap::new();
        let mut type_counts: BTreeMap<&str, usize> = BTreeMap::new();
        for node_idx in self.samples.keys().copied() {
            if let Some(info) = self.node_info.get(node_idx) {
                *type_counts.entry(info.type_id.as_str()).or_insert(0) += 1;
            }
        }
        let mut type_instance: BTreeMap<&str, usize> = BTreeMap::new();

        for (node_idx, deque) in &self.samples {
            if deque.is_empty() {
                continue;
            }
            let info = self.node_info.get(*node_idx);
            let type_id = info.map(|info| info.type_id.as_str()).unwrap_or("unknown");
            let count = type_counts.get(type_id).copied().unwrap_or(0);
            let key = if count <= 1 {
                type_id.to_string()
            } else {
                let idx = type_instance.entry(type_id).or_insert(0);
                *idx += 1;
                format!("{type_id}#{idx}")
            };

            let sample_count = deque.len() as u64;
            let sum_ms: f64 = deque.iter().map(|(_, ms)| *ms).sum();
            let average_time_ms = sum_ms / sample_count.max(1) as f64;
            let (first_t, _) = deque.front().copied().unwrap_or((now, 0.0));
            let (last_t, _) = deque.back().copied().unwrap_or((now, 0.0));
            let elapsed = last_t.saturating_duration_since(first_t).as_secs_f64();
            let average_fps = if elapsed > 0.000_001 && sample_count > 1 { (sample_count as f64 - 1.0) / elapsed } else { 0.0 };
            let last_sample_age_ms = Some(now.saturating_duration_since(last_t).as_millis() as u64);
            let perf = self.node_perf_samples.get(node_idx).and_then(|perf_deque| {
                if perf_deque.is_empty() {
                    return None;
                }
                let perf_sample_count = perf_deque.len() as u64;
                let sum_cache: f64 = perf_deque.iter().map(|sample| sample.cache_misses).sum();
                let sum_branch_inst: f64 = perf_deque.iter().map(|sample| sample.branch_instructions).sum();
                let sum_branch_miss: f64 = perf_deque.iter().map(|sample| sample.branch_misses).sum();
                let last_age_ms = self.node_perf_last_at.get(node_idx).map(|instant| now.saturating_duration_since(*instant).as_millis() as u64);
                Some(PipelineNodePerfMetrics {
                    average_cache_misses: sum_cache / perf_sample_count.max(1) as f64,
                    average_branch_instructions: sum_branch_inst / perf_sample_count.max(1) as f64,
                    average_branch_misses: sum_branch_miss / perf_sample_count.max(1) as f64,
                    sample_count: perf_sample_count,
                    window_size: self.window as u64,
                    last_sample_age_ms: last_age_ms,
                })
            });
            let payload = self.node_payload_samples.get(node_idx).map(&summarize_payload);
            out.insert(
                key,
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms, average_fps, sample_count, window_size: self.window as u64, last_sample_age_ms },
                    perf,
                    average_input_payload_bytes: payload.map(|p| p.0).unwrap_or(0.0),
                    average_output_payload_bytes: payload.map(|p| p.1).unwrap_or(0.0),
                    peak_input_payload_bytes: payload.map(|p| p.2).unwrap_or(0),
                    peak_output_payload_bytes: payload.map(|p| p.3).unwrap_or(0),
                    peak_payload_working_set_bytes: payload.map(|p| p.4).unwrap_or(0),
                    children: None,
                    node_type: info.map(|info| info.type_id.clone()),
                    node_label: info.and_then(|info| info.label.clone()),
                    node_index: Some(*node_idx as u64),
                    retained_output_sample_count: 0,
                    retained_output_sample_bytes: 0,
                    retained_output_ports: None,
                    last_error: info.and_then(|info| self.last_errors.get(&info.type_id)).map(|(_, message)| message.clone()),
                    last_error_at: info.and_then(|info| self.last_errors.get(&info.type_id)).map(|(instant, _)| now.saturating_duration_since(*instant).as_millis() as u64),
                },
            );
        }
        if !self.graph_samples.is_empty() {
            let deque = &self.graph_samples;
            let sample_count = deque.len() as u64;
            let sum_ms: f64 = deque.iter().map(|(_, ms)| *ms).sum();
            let average_time_ms = sum_ms / sample_count.max(1) as f64;
            let (first_t, _) = deque.front().copied().unwrap_or((now, 0.0));
            let (last_t, _) = deque.back().copied().unwrap_or((now, 0.0));
            let elapsed = last_t.saturating_duration_since(first_t).as_secs_f64();
            let average_fps = if elapsed > 0.000_001 && sample_count > 1 { (sample_count as f64 - 1.0) / elapsed } else { 0.0 };
            let last_sample_age_ms = Some(now.saturating_duration_since(last_t).as_millis() as u64);
            out.insert(
                "graph".to_string(),
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms, average_fps, sample_count, window_size: self.window as u64, last_sample_age_ms },
                    perf: None,
                    average_input_payload_bytes: 0.0,
                    average_output_payload_bytes: 0.0,
                    peak_input_payload_bytes: 0,
                    peak_output_payload_bytes: 0,
                    peak_payload_working_set_bytes: 0,
                    children: None,
                    node_type: Some("graph".to_string()),
                    node_label: Some("graph".to_string()),
                    node_index: None,
                    retained_output_sample_count: 0,
                    retained_output_sample_bytes: 0,
                    retained_output_ports: None,
                    last_error: None,
                    last_error_at: None,
                },
            );
        }
        for (node_type, (instant, message)) in &self.last_errors {
            if out.contains_key(node_type) {
                continue;
            }
            let label = self.node_info.iter().find(|info| info.type_id == *node_type).and_then(|info| info.label.clone());
            out.insert(
                node_type.clone(),
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms: 0.0, average_fps: 0.0, sample_count: 0, window_size: self.window as u64, last_sample_age_ms: None },
                    perf: None,
                    average_input_payload_bytes: 0.0,
                    average_output_payload_bytes: 0.0,
                    peak_input_payload_bytes: 0,
                    peak_output_payload_bytes: 0,
                    peak_payload_working_set_bytes: 0,
                    children: None,
                    node_type: Some(node_type.clone()),
                    node_label: label,
                    node_index: None,
                    retained_output_sample_count: 0,
                    retained_output_sample_bytes: 0,
                    retained_output_ports: None,
                    last_error: Some(message.clone()),
                    last_error_at: Some(now.saturating_duration_since(*instant).as_millis() as u64),
                },
            );
        }

        if let Some((instant, message)) = self.warnings.back() {
            let entry = out.entry("graph".to_string()).or_insert(PipelineNodeRuntimeMetrics {
                metrics: PipelineNodeMetrics { average_time_ms: 0.0, average_fps: 0.0, sample_count: 0, window_size: self.window as u64, last_sample_age_ms: None },
                perf: None,
                average_input_payload_bytes: 0.0,
                average_output_payload_bytes: 0.0,
                peak_input_payload_bytes: 0,
                peak_output_payload_bytes: 0,
                peak_payload_working_set_bytes: 0,
                children: None,
                node_type: Some("graph".to_string()),
                node_label: Some("graph".to_string()),
                node_index: None,
                retained_output_sample_count: 0,
                retained_output_sample_bytes: 0,
                retained_output_ports: None,
                last_error: None,
                last_error_at: None,
            });
            entry.last_error = Some(message.clone());
            entry.last_error_at = Some(now.saturating_duration_since(*instant).as_millis() as u64);
        }

        let mut group_entries: BTreeMap<String, PipelineNodeRuntimeMetrics> = BTreeMap::new();
        for (group_id, deque) in &self.group_samples {
            if deque.is_empty() {
                continue;
            }
            let sample_count = deque.len() as u64;
            let sum_ms: f64 = deque.iter().map(|(_, ms)| *ms).sum();
            let average_time_ms = sum_ms / sample_count.max(1) as f64;
            let (first_t, _) = deque.front().copied().unwrap_or((now, 0.0));
            let (last_t, _) = deque.back().copied().unwrap_or((now, 0.0));
            let elapsed = last_t.saturating_duration_since(first_t).as_secs_f64();
            let average_fps = if elapsed > 0.000_001 && sample_count > 1 { (sample_count as f64 - 1.0) / elapsed } else { 0.0 };
            let last_sample_age_ms = Some(now.saturating_duration_since(last_t).as_millis() as u64);
            let perf = self.group_perf_samples.get(group_id).and_then(|perf_deque| {
                if perf_deque.is_empty() {
                    return None;
                }
                let perf_sample_count = perf_deque.len() as u64;
                let sum_cache: f64 = perf_deque.iter().map(|sample| sample.cache_misses).sum();
                let sum_branch_inst: f64 = perf_deque.iter().map(|sample| sample.branch_instructions).sum();
                let sum_branch_miss: f64 = perf_deque.iter().map(|sample| sample.branch_misses).sum();
                let last_age_ms = self.group_perf_last_at.get(group_id).map(|instant| now.saturating_duration_since(*instant).as_millis() as u64);
                Some(PipelineNodePerfMetrics {
                    average_cache_misses: sum_cache / perf_sample_count.max(1) as f64,
                    average_branch_instructions: sum_branch_inst / perf_sample_count.max(1) as f64,
                    average_branch_misses: sum_branch_miss / perf_sample_count.max(1) as f64,
                    sample_count: perf_sample_count,
                    window_size: self.window as u64,
                    last_sample_age_ms: last_age_ms,
                })
            });
            let payload = self.group_payload_samples.get(group_id).map(&summarize_payload);
            group_entries.insert(
                group_id.clone(),
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms, average_fps, sample_count, window_size: self.window as u64, last_sample_age_ms },
                    perf,
                    average_input_payload_bytes: payload.map(|p| p.0).unwrap_or(0.0),
                    average_output_payload_bytes: payload.map(|p| p.1).unwrap_or(0.0),
                    peak_input_payload_bytes: payload.map(|p| p.2).unwrap_or(0),
                    peak_output_payload_bytes: payload.map(|p| p.3).unwrap_or(0),
                    peak_payload_working_set_bytes: payload.map(|p| p.4).unwrap_or(0),
                    children: None,
                    node_type: Some("group".to_string()),
                    node_label: Some(group_id.clone()),
                    node_index: None,
                    retained_output_sample_count: 0,
                    retained_output_sample_bytes: 0,
                    retained_output_ports: None,
                    last_error: None,
                    last_error_at: None,
                },
            );
        }

        let mut grouped_nodes: BTreeMap<String, BTreeMap<String, PipelineNodeRuntimeMetrics>> = BTreeMap::new();
        for (node_key, metrics) in &out {
            let Some(node_index) = metrics.node_index else {
                continue;
            };
            let Some(info) = self.node_info.get(node_index as usize) else {
                continue;
            };
            let Some(group) = info.group.as_ref() else {
                continue;
            };
            grouped_nodes.entry(group.clone()).or_default().insert(node_key.clone(), metrics.clone());
        }

        for (group_id, children) in grouped_nodes {
            let entry = group_entries.entry(group_id.clone()).or_insert(PipelineNodeRuntimeMetrics {
                metrics: PipelineNodeMetrics { average_time_ms: 0.0, average_fps: 0.0, sample_count: 0, window_size: self.window as u64, last_sample_age_ms: None },
                perf: None,
                average_input_payload_bytes: 0.0,
                average_output_payload_bytes: 0.0,
                peak_input_payload_bytes: 0,
                peak_output_payload_bytes: 0,
                peak_payload_working_set_bytes: 0,
                children: None,
                node_type: Some("group".to_string()),
                node_label: Some(group_id.clone()),
                node_index: None,
                retained_output_sample_count: 0,
                retained_output_sample_bytes: 0,
                retained_output_ports: None,
                last_error: None,
                last_error_at: None,
            });
            entry.children = Some(children);
        }

        let mut root_groups: BTreeMap<String, PipelineNodeRuntimeMetrics> = BTreeMap::new();
        if !group_entries.is_empty() {
            let group_ids: BTreeSet<String> = group_entries.keys().cloned().collect();
            let mut child_groups: BTreeSet<String> = BTreeSet::new();
            for group_id in &group_ids {
                if let Some((parent, _)) = group_id.rsplit_once("::") {
                    if group_ids.contains(parent) {
                        child_groups.insert(group_id.clone());
                    }
                }
            }
            for child_id in &child_groups {
                if let Some((parent, _)) = child_id.rsplit_once("::") {
                    if let Some(child_entry) = group_entries.get(child_id).cloned() {
                        if let Some(parent_entry) = group_entries.get_mut(parent) {
                            parent_entry.children.get_or_insert_with(BTreeMap::new).insert(child_id.clone(), child_entry);
                        }
                    }
                }
            }
            for group_id in &group_ids {
                if !child_groups.contains(group_id) {
                    if let Some(entry) = group_entries.get(group_id).cloned() {
                        root_groups.insert(group_id.clone(), entry);
                    }
                }
            }
        }

        let mut edge_entries: BTreeMap<String, crate::stream::PipelineEdgeRuntimeMetrics> = BTreeMap::new();
        for (edge_idx, deque) in &self.edge_samples {
            if deque.is_empty() {
                continue;
            }

            let wait_sample_count: u64 = deque.iter().map(|metrics| metrics.samples as u64).sum();
            let total_wait_ms: f64 = deque.iter().map(|metrics| metrics.total_wait.as_secs_f64() * 1000.0).sum();
            let payload_count: u64 = deque.iter().map(|metrics| metrics.transport_count).sum();
            let payload_bytes: u64 = deque.iter().map(|metrics| metrics.transport_bytes).sum();
            let max_depth = deque.iter().map(|metrics| metrics.max_depth).max().unwrap_or(0);
            let dropped = deque.iter().map(|metrics| metrics.drops).sum();
            let gpu_uploads = deque.iter().map(|metrics| metrics.gpu_uploads).sum();
            let gpu_downloads = deque.iter().map(|metrics| metrics.gpu_downloads).sum();
            let current_depth = deque.back().map(|metrics| metrics.current_depth).unwrap_or(0);
            let average_wait_ms = if wait_sample_count > 0 { total_wait_ms / wait_sample_count as f64 } else { 0.0 };
            let average_payload_bytes = if payload_count > 0 { payload_bytes as f64 / payload_count as f64 } else { 0.0 };
            let last_sample_age_ms = self.edge_last_at.get(edge_idx).map(|instant| now.saturating_duration_since(*instant).as_millis() as u64);
            let info = self.edge_info.get(*edge_idx);
            let capacity = deque.iter().filter_map(|metrics| metrics.capacity).max().or_else(|| info.and_then(|edge| edge.queue_capacity));

            edge_entries.insert(
                format!("edge_{edge_idx}"),
                crate::stream::PipelineEdgeRuntimeMetrics {
                    average_wait_ms,
                    wait_sample_count,
                    window_size: self.window as u64,
                    last_sample_age_ms,
                    max_depth,
                    current_depth,
                    current_queue_bytes: deque.back().map(|metrics| metrics.current_queue_bytes).unwrap_or(0),
                    peak_queue_bytes: deque.iter().map(|metrics| metrics.peak_queue_bytes).max().unwrap_or(0),
                    capacity,
                    dropped,
                    payload_bytes,
                    payload_count,
                    average_payload_bytes,
                    gpu_uploads,
                    gpu_downloads,
                    edge_index: *edge_idx as u64,
                    from_node_index: info.map(|edge| edge.from_node_index as u64),
                    from_node_label: info.and_then(|edge| edge.from_node_label.clone()),
                    from_port: info.map(|edge| edge.from_port.clone()),
                    to_node_index: info.map(|edge| edge.to_node_index as u64),
                    to_node_label: info.and_then(|edge| edge.to_node_label.clone()),
                    to_port: info.map(|edge| edge.to_port.clone()),
                    policy: info.map(|edge| edge.policy.clone()),
                },
            );
        }

        let perf = if self.perf_samples.is_empty() {
            None
        } else {
            let sample_count = self.perf_samples.len() as u64;
            let sum_cache: f64 = self.perf_samples.iter().map(|sample| sample.cache_misses as f64).sum();
            let sum_branch_inst: f64 = self.perf_samples.iter().map(|sample| sample.branch_instructions as f64).sum();
            let sum_branch_miss: f64 = self.perf_samples.iter().map(|sample| sample.branch_misses as f64).sum();
            let last_age_ms = self.perf_last_at.map(|instant| now.saturating_duration_since(instant).as_millis() as u64);
            Some(PipelinePerfMetrics {
                average_cache_misses: sum_cache / sample_count.max(1) as f64,
                average_branch_instructions: sum_branch_inst / sample_count.max(1) as f64,
                average_branch_misses: sum_branch_miss / sample_count.max(1) as f64,
                sample_count,
                window_size: self.window as u64,
                last_sample_age_ms: last_age_ms,
            })
        };

        let flamegraph = self.last_flamegraph.as_ref().map(|capture| PipelineFlamegraphMetrics { path: capture.path.clone(), size_bytes: capture.size_bytes, captured_at_ms: capture.captured_at_ms });
        let warnings =
            self.warnings.iter().rev().filter(|(instant, _)| now.saturating_duration_since(*instant) <= Duration::from_secs(4)).map(|(_, warning)| warning.clone()).take(4).collect::<Vec<_>>();

        PipelineGraphMetrics {
            nodes: out,
            groups: if root_groups.is_empty() { None } else { Some(root_groups) },
            edges: if edge_entries.is_empty() { None } else { Some(edge_entries) },
            sample_cache: None,
            image_working_set: None,
            executor: {
                if self.graph_samples.is_empty() {
                    None
                } else {
                    let sample_count = self.graph_samples.len() as u64;
                    let sum_ms: f64 = self.graph_samples.iter().map(|(_, ms)| *ms).sum();
                    let average_time_ms = sum_ms / sample_count.max(1) as f64;
                    let (last_t, last_time_ms) = self.graph_samples.back().copied().unwrap_or((now, 0.0));
                    Some(crate::stream::PipelineTimingMetrics {
                        average_time_ms,
                        last_time_ms,
                        sample_count,
                        window_size: self.window as u64,
                        last_sample_age_ms: Some(now.saturating_duration_since(last_t).as_millis() as u64),
                    })
                }
            },
            wrapper: summarize_timing(&self.wrapper_samples, self.wrapper_last_at),
            executor_lock: summarize_timing(&self.lock_samples, self.lock_last_at),
            output_materialization: summarize_timing(&self.output_materialization_samples, self.output_materialization_last_at),
            perf,
            flamegraph,
            warnings,
        }
    }
}

fn is_internal_runtime_warning(warning: &str) -> bool {
    let trimmed = warning.trim();
    // Daedalus may emit internal segment identifiers for GPU-preferred paths that
    // safely fallback to CPU. These are implementation details and should not be
    // surfaced as user-facing "Pipeline warning" banners.
    trimmed.starts_with("gpu_preferred_fallback_cpu_seg_")
}

fn should_retain_structured_output_port(port: &str, requested_sample_ports: &BTreeSet<String>, auto_target_roi_source_port: Option<&str>, has_preview_ports: bool) -> bool {
    if !has_preview_ports {
        return true;
    }
    requested_sample_ports.contains(port) || auto_target_roi_source_port.is_some_and(|source| source.eq_ignore_ascii_case(port))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutorBusyBehavior {
    Drop,
    Block,
}

impl DaedalusGraphExecutor {
    fn new(graph: Graph, pool_size: Option<usize>, output_port: Option<String>) -> Result<Self, GraphError> {
        let mut graph = graph;
        let host_mgr = DaedalusBridgeManager::new();
        let built = build_daedalus_runtime_registry(&host_mgr, Some(&graph)).map_err(|e| GraphError::Build(e.to_string()))?;
        let (registry, handlers, _plugins) = built.into_parts();
        let const_coercers = registry.const_coercers.clone();
        let output_movers = registry.output_movers.clone();

        sync_graph_node_port_declarations(&mut graph, &registry);
        enforce_registry_default_compute_affinity(&mut graph, &registry);
        canonicalize_graph_const_inputs(&mut graph, &registry);
        if std::env::var_os("HELIOS_TRACE_GRAPH_CONSTS_STDERR").is_some() {
            for node in &graph.nodes {
                if node.id.0 == "cv:image:blur" || node.id.0 == "cv:color:grayscale" {
                    eprintln!("helios-engine: node consts id={} const_inputs={:?}", node.id.0, node.const_inputs);
                }
            }
        }

        let mut cfg = EngineConfig::default();
        apply_daedalus_engine_config_overrides(&mut cfg, &graph);
        let gpu_backend_overridden = graph.metadata.contains_key("helios.daedalus.gpu_backend");
        let planner_gpu_overridden = graph.metadata.contains_key("helios.daedalus.planner.enable_gpu");
        let runtime_policy_overridden = graph.metadata.contains_key(KEY_RUNTIME_DEFAULT_POLICY);
        let runtime_backpressure_overridden = graph.metadata.contains_key(KEY_RUNTIME_BACKPRESSURE);
        let graph_requests_gpu = graph.nodes.iter().any(|node| !matches!(node.compute, ComputeAffinity::CpuOnly));
        if graph_requests_gpu {
            if !gpu_backend_overridden {
                cfg.gpu = daedalus::engine::GpuBackend::Device;
            }
            if !planner_gpu_overridden {
                cfg.planner.enable_gpu = true;
            }
            if !runtime_policy_overridden {
                cfg.runtime.default_policy = EdgePolicyKind::Bounded { cap: policy::runtime_queue_cap() };
            }
            if !runtime_backpressure_overridden {
                cfg.runtime.backpressure = BackpressureStrategy::BoundedQueues;
            }
        }
        apply_daedalus_engine_env_overrides(&mut cfg);
        let engine = Engine::new(cfg).map_err(|e| GraphError::Build(e.to_string()))?;

        let graph_has_calibration = graph.nodes.iter().any(|node| {
            let id = node.id.0.as_str();
            if !(id == "io.host_bridge" || id.ends_with(":io.host_bridge")) {
                return false;
            }
            node.outputs.iter().any(|p| p.eq_ignore_ascii_case("calibration"))
        });

        let mut declared_host_bridge_ports: BTreeSet<String> = BTreeSet::new();
        for node in graph.nodes.iter().filter(|node| {
            let id = node.id.0.as_str();
            id == "io.host_bridge" || id.ends_with(":io.host_bridge")
        }) {
            for port in &node.outputs {
                let trimmed = port.trim();
                if trimmed.is_empty() {
                    continue;
                }
                declared_host_bridge_ports.insert(trimmed.to_string());
            }
        }

        // Host output ports are a graph contract: the authoritative port list comes from the graph
        // JSON node's declared `inputs` array (not from runtime inference).
        let mut declared_host_output_ports: Vec<String> = Vec::new();
        let mut declared_host_output_ports_lc: BTreeSet<String> = BTreeSet::new();
        for node in graph.nodes.iter().filter(|node| {
            let id = node.id.0.as_str();
            id == "io.host_output" || id.ends_with(":io.host_output")
        }) {
            for port in &node.inputs {
                let trimmed = port.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let lc = trimmed.to_ascii_lowercase();
                if declared_host_output_ports_lc.insert(lc) {
                    declared_host_output_ports.push(trimmed.to_string());
                }
            }
        }
        let graph_has_color_sensitive_nodes = graph.nodes.iter().any(|node| node_requires_color_input(node.id.0.as_str()));
        let graph_auto_target_roi_enabled = metadata_bool_flag(&graph.metadata, "helios.auto_target_roi").unwrap_or_else(policy::auto_target_roi_enabled);
        let planner_output = engine.plan(&registry.registry, graph).map_err(|e| GraphError::Build(e.to_string()))?;
        let runtime_plan = engine.build_runtime_plan(&planner_output.plan).map_err(|e| GraphError::Build(e.to_string()))?;
        host_mgr.populate_from_plan(&runtime_plan);
        // Ensure host-output incoming port types are available even when planner metadata is
        // incomplete for dynamic-input nodes. We can infer them from source-node output metadata.
        for (alias, incoming) in infer_host_output_incoming_types(&runtime_plan, &registry.registry) {
            host_mgr.register_port_types(alias, std::iter::empty::<(String, DaedalusTypeExpr)>(), incoming);
        }
        let (input_host_alias, input_port, output_hosts) = derive_host_aliases(&runtime_plan, &host_mgr)?;

        // The host bridge manager now owns solved types (from planning/runtime plan). Do not
        // accept any type hints from the user graph JSON; only use what the planner inferred.
        let mut calibration_port = host_mgr.handle(&input_host_alias).and_then(|handle| handle.outgoing_ports().find(|p| p.eq_ignore_ascii_case("calibration")).map(|p| p.to_string()));
        if calibration_port.is_none() && graph_has_calibration {
            calibration_port = Some("calibration".to_string());
        }

        // Solved types for declared host output ports (keyed by normalized name).
        let mut host_output_port_types: BTreeMap<String, DaedalusTypeExpr> = BTreeMap::new();
        for port in &declared_host_output_ports {
            let target = port.trim();
            if target.is_empty() {
                continue;
            }
            let mut found: Option<DaedalusTypeExpr> = None;
            for alias in &output_hosts {
                let Some(host) = host_mgr.handle(alias) else { continue };
                if let Some(p) = host.incoming_ports().find(|p| p.name().eq_ignore_ascii_case(target)) {
                    found = p.resolved_type().cloned();
                    break;
                }
            }
            if let Some(ty) = found {
                host_output_port_types.insert(target.to_ascii_lowercase(), ty);
            }
        }

        // Previewable image ports are those declared by the graph and solved as image payloads.
        let mut previewable: Vec<String> = Vec::new();
        for port in &declared_host_output_ports {
            let key = port.to_ascii_lowercase();
            if let Some(ty) = host_output_port_types.get(&key) {
                if is_image_payload(ty) {
                    previewable.push(port.clone());
                }
            }
        }

        // Default preview selection:
        // - selected output must be declared and must solve to an image payload.
        // - otherwise, use the first previewable port in graph-declared order (if any).
        let mut preview_ports: Vec<String> = Vec::new();
        if let Some(selected) = output_port {
            let selected_known = declared_host_output_ports.iter().any(|p| p.eq_ignore_ascii_case(&selected));
            if !selected_known {
                return Err(GraphError::Build(format!("selected output {:?} not found in graph-declared host outputs {:?}", selected, declared_host_output_ports)));
            }
            let key = selected.trim().to_ascii_lowercase();
            if let Some(ty) = host_output_port_types.get(&key) {
                if !is_image_payload(ty) {
                    return Err(GraphError::Build(format!("selected output {:?} is not an image payload (solved type {:?})", selected, ty)));
                }
            } else {
                // Runtime type propagation for host bridge outputs can be unavailable on cold-start
                // graphs. Allow the explicit output selection and rely on runtime decode attempts.
                tracing::warn!(selected_output = %selected, "selected output has unknown solved type; continuing with best-effort image decode");
            }
            let canonical = declared_host_output_ports.iter().find(|p| p.eq_ignore_ascii_case(&selected)).cloned().unwrap_or(selected);
            preview_ports = vec![canonical];
        } else if let Some(first) = previewable.first().cloned() {
            preview_ports = vec![first];
        }

        let host_output_ports = declared_host_output_ports;
        let host_output_ports_lc: BTreeSet<String> = host_output_ports.iter().map(|p| p.to_ascii_lowercase()).collect();
        let host_output_port_owners = infer_host_output_port_owners(&runtime_plan, &output_hosts);
        let prefers_grayscale_input = graph_prefers_grayscale_input(graph_has_color_sensitive_nodes, &preview_ports, &host_output_port_types);
        let plan = Arc::new(runtime_plan);
        let node_info: Vec<NodeInfo> = plan
            .nodes
            .iter()
            .map(|node| {
                let group = node.metadata.get("daedalus.embedded_group").and_then(|value| match value {
                    DaedalusValue::String(group) => {
                        let trimmed = group.as_ref().trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        }
                    }
                    _ => None,
                });
                NodeInfo { type_id: node.id.clone(), label: node.label.clone(), group }
            })
            .collect();
        let edge_info: Vec<EdgeInfo> = plan
            .edges
            .iter()
            .map(|(from_node, from_port, to_node, to_port, policy)| EdgeInfo {
                from_node_index: from_node.0,
                from_node_label: node_info.get(from_node.0).and_then(|info| info.label.clone()).or_else(|| node_info.get(from_node.0).map(|info| info.type_id.clone())),
                from_port: from_port.clone(),
                to_node_index: to_node.0,
                to_node_label: node_info.get(to_node.0).and_then(|info| info.label.clone()).or_else(|| node_info.get(to_node.0).map(|info| info.type_id.clone())),
                to_port: to_port.clone(),
                queue_capacity: match policy {
                    EdgePolicyKind::Bounded { cap } => Some(*cap as u64),
                    _ => None,
                },
                policy: format!("{policy:?}"),
            })
            .collect();

        let gpu = match engine.config().gpu {
            daedalus::engine::GpuBackend::Cpu => None,
            daedalus::engine::GpuBackend::Mock => {
                let opts = GpuOptions { preferred_backend: Some(GpuBackendKind::Mock), adapter_label: None, allow_software: true };
                match select_backend(&opts) {
                    Ok(handle) => {
                        tracing::info!(backend = ?handle.backend_kind(), adapter = %handle.adapter_info().name, skipped = ?handle.skipped_summary(), "daedalus gpu backend selected");
                        if handle.backend_kind() == GpuBackendKind::Noop {
                            return Err(GraphError::Build(format!("gpu backend requested but unavailable (mock); skipped={:?}", handle.skipped_summary())));
                        } else {
                            Some(handle)
                        }
                    }
                    Err(e) => {
                        return Err(GraphError::Build(format!("gpu backend init failed: {e}")));
                    }
                }
            }
            daedalus::engine::GpuBackend::Device => {
                let opts = GpuOptions { preferred_backend: Some(GpuBackendKind::Wgpu), adapter_label: None, allow_software: false };
                match select_backend(&opts) {
                    Ok(handle) => {
                        tracing::info!(backend = ?handle.backend_kind(), adapter = %handle.adapter_info().name, skipped = ?handle.skipped_summary(), "daedalus gpu backend selected");
                        if handle.backend_kind() == GpuBackendKind::Noop {
                            return Err(GraphError::Build(format!("gpu backend requested but unavailable; skipped={:?}", handle.skipped_summary())));
                        } else {
                            Some(handle)
                        }
                    }
                    Err(e) => {
                        return Err(GraphError::Build(format!("gpu backend init failed: {e}")));
                    }
                }
            }
        };

        if let Some(handle) = gpu.clone() {
            host_mgr.attach_gpu(handle);
        }

        let gpu_plan_active = gpu.is_some() && plan_uses_gpu(plan.as_ref());
        let host_outputs_in_graph = policy::host_outputs_in_graph_enabled(Some(plan.as_ref()), gpu_plan_active);
        let demand_driven = policy::demand_driven_enabled(Some(plan.as_ref()), gpu_plan_active);
        let run_metrics_level = engine.config().runtime.metrics_level;
        let roi_ports_present = declared_host_bridge_ports.contains("roi_x")
            && declared_host_bridge_ports.contains("roi_y")
            && declared_host_bridge_ports.contains("roi_w")
            && declared_host_bridge_ports.contains("roi_h");
        let auto_target_roi_source_port = if graph_auto_target_roi_enabled && roi_ports_present { host_output_detection_source_port(&host_output_ports) } else { None };
        let preview_aux_ports = auto_target_roi_source_port.iter().cloned().collect::<Vec<_>>();
        let active_nodes_with_image =
            build_demand_mask(plan.as_ref(), &output_hosts, &preview_ports, &host_output_ports, &host_output_port_types, &host_output_port_owners, demand_driven, true, true, &[]).map(Arc::new);
        let active_nodes_preview_only =
            build_demand_mask(plan.as_ref(), &output_hosts, &preview_ports, &host_output_ports, &host_output_port_types, &host_output_port_owners, demand_driven, true, false, &preview_aux_ports)
                .map(Arc::new);
        let active_nodes_without_image =
            build_demand_mask(plan.as_ref(), &output_hosts, &preview_ports, &host_output_ports, &host_output_port_types, &host_output_port_owners, demand_driven, false, true, &[]).map(Arc::new);
        let mut executor = DaedalusOwnedExecutor::new(plan.clone(), handlers.clone_arc())
            .with_host_bridges(host_mgr.clone())
            .with_const_coercers(const_coercers.clone())
            .with_output_movers(output_movers.clone())
            // Daedalus error-isolation: keep the graph running and surface errors via telemetry
            // instead of killing the whole run on the first failing node.
            .with_fail_fast(false)
            .with_metrics_level(run_metrics_level)
            // Host output execution can be moved "in graph" for responsiveness, but this changes
            // scheduling semantics and can cause missing outputs depending on executor ordering.
            // Keep it opt-in until Daedalus scheduling guarantees sink ordering.
            .with_host_outputs_in_graph(host_outputs_in_graph);

        let preview_ports_lc: BTreeSet<String> = preview_ports.iter().map(|p| p.to_ascii_lowercase()).collect();
        if let Some(mask) = active_nodes_with_image.clone() {
            executor = executor.with_active_nodes_mask(Some(mask));
        }
        if let Some(handle) = gpu.clone() {
            executor = executor.with_gpu(handle);
        }
        if let Some(size) = pool_size {
            executor = executor.with_pool_size(Some(size));
        }
        let dedicated_executor = policy::dedicated_executor();
        let busy_behavior = policy::executor_busy_behavior();
        let busy_timeout = policy::executor_busy_timeout();

        tracing::info!(
            input_host = %input_host_alias,
            input_port = %input_port,
            output_hosts = ?output_hosts,
            host_output_ports = ?host_output_ports,
            preview_ports = ?preview_ports,
            "daedalus graph: host ports configured"
        );

        let mut seeded_input_values: BTreeMap<String, DaedalusValue> = BTreeMap::new();
        for port in &declared_host_bridge_ports {
            if port.eq_ignore_ascii_case(&input_port) {
                continue;
            }
            if calibration_port.as_deref().is_some_and(|cal| port.eq_ignore_ascii_case(cal)) {
                continue;
            }
            let key = port.to_ascii_lowercase();
            let Some(default_value) = default_host_bridge_input_value(&key) else {
                continue;
            };
            seeded_input_values.insert(key, default_value);
        }

        let pprof_enabled = policy::pprof_enabled();
        let pprof_duration_ms = if pprof_enabled { policy::pprof_duration_ms() } else { None };
        let pprof_until_ms = pprof_duration_ms.and_then(|d| now_ms().checked_add(d)).unwrap_or(0);
        Ok(Self {
            plan,
            handlers,
            pool_size,
            const_coercers,
            output_movers,
            dedicated_executor,
            busy_behavior,
            busy_timeout,
            host_mgr,
            gpu,
            input_host_alias,
            input_port,
            calibration_port,
            declared_input_ports_lc: declared_host_bridge_ports.iter().map(|port| port.to_ascii_lowercase()).collect(),
            output_hosts,
            host_output_ports,
            host_output_ports_lc,
            host_output_port_types,
            host_output_port_owners,
            preview_ports,
            preview_ports_lc,
            preview_aux_ports: preview_aux_ports.clone(),
            preview_aux_ports_lc: preview_aux_ports.iter().map(|port| port.to_ascii_lowercase()).collect(),
            prefers_grayscale_input,
            run_mode: engine.config().runtime.mode.clone(),
            run_metrics_level,
            active_nodes_with_image,
            active_nodes_preview_only,
            active_nodes_without_image,
            executor: Arc::new(std::sync::Mutex::new(executor)),
            metrics: Mutex::new(RollingGraphMetrics::new(node_metrics_window(run_metrics_level), node_info, edge_info)),
            value_samples: Mutex::new(BTreeMap::new()),
            typed_samples: Mutex::new(BTreeMap::new()),
            image_samples: Mutex::new(BTreeMap::new()),
            requested_sample_ports: Mutex::new(BTreeMap::new()),
            image_working_set: GraphImageWorkingSetTracker::default(),
            process_calls: AtomicU64::new(0),
            perf_enabled: AtomicBool::new(policy::perf_counters_enabled()),
            pprof_pending: AtomicBool::new(pprof_enabled),
            pprof_remaining: AtomicU64::new(if pprof_enabled && pprof_until_ms == 0 { policy::pprof_frames() } else { 0 }),
            pprof_until_ms: AtomicU64::new(pprof_until_ms),
            pprof_guard: Mutex::new(None),
            calibration_payload: std::sync::RwLock::new(calibration_to_daedalus_value(None)),
            default_input_values: seeded_input_values.clone(),
            input_values: std::sync::RwLock::new(seeded_input_values),
            auto_target_roi_source_port,
            auto_target_roi: Mutex::new(AutoTargetRoiState::default()),
            last_background_trim_ms: AtomicU64::new(0),
            last_error_detail: std::sync::RwLock::new(String::new()),
            failure_count: AtomicU64::new(0),
            disabled: AtomicBool::new(false),
            disabled_since_ms: AtomicU64::new(0),
            rebuild_requested: AtomicBool::new(false),
        })
    }

    fn rebuild_shared_executor(&self) -> Result<(), String> {
        let gpu_plan_active = self.gpu.is_some() && plan_uses_gpu(self.plan.as_ref());
        let host_outputs_in_graph = policy::host_outputs_in_graph_enabled(Some(self.plan.as_ref()), gpu_plan_active);
        let mut executor = DaedalusOwnedExecutor::new(self.plan.clone(), self.handlers.clone_arc())
            .with_host_bridges(self.host_mgr.clone())
            .with_const_coercers(self.const_coercers.clone())
            .with_output_movers(self.output_movers.clone())
            .with_fail_fast(false)
            .with_metrics_level(self.run_metrics_level)
            .with_host_outputs_in_graph(host_outputs_in_graph);
        if let Some(mask) = self.active_nodes_with_image.clone() {
            executor = executor.with_active_nodes_mask(Some(mask));
        }
        if let Some(handle) = self.gpu.clone() {
            executor = executor.with_gpu(handle);
        }
        if let Some(size) = self.pool_size {
            executor = executor.with_pool_size(Some(size));
        }
        let mut guard = self.executor.lock().unwrap_or_else(PoisonError::into_inner);
        *guard = executor;
        Ok(())
    }
}

impl GraphExecutor for DaedalusGraphExecutor {
    fn set_calibration(&self, calibration: Option<crate::ipc::StreamCalibration>) {
        if let Ok(mut guard) = self.calibration_payload.write() {
            *guard = calibration_to_daedalus_value(calibration);
        }
    }

    fn set_pipeline_inputs(&self, _pipeline_id: Option<uuid::Uuid>, inputs: &BTreeMap<String, Option<DaedalusValue>>) {
        if inputs.is_empty() {
            return;
        }
        if let Ok(mut guard) = self.input_values.write() {
            for (port, value) in inputs {
                let key = port.trim().to_ascii_lowercase();
                if key.is_empty() {
                    continue;
                }
                if let Some(value) = value {
                    guard.insert(key, value.clone());
                } else if let Some(default_value) = self.default_input_values.get(&key) {
                    guard.insert(key, default_value.clone());
                } else {
                    guard.remove(&key);
                }
            }
        }
    }

    fn apply_graph_patch(&self, _pipeline_id: Option<uuid::Uuid>, patch: &GraphPatch) -> Option<PatchReport> {
        let guard = self.executor.lock().unwrap_or_else(PoisonError::into_inner);
        Some(guard.apply_patch(patch))
    }

    fn process(&self, image: DynamicImage) -> Option<DynamicImage> {
        self.process_with_options(image, GraphProcessOptions::default())
    }

    fn process_with_options(&self, image: DynamicImage, options: GraphProcessOptions) -> Option<DynamicImage> {
        self.process_preview_with_options(image, options).map(GraphPreviewOutput::into_dynamic_image)
    }

    fn process_preview_with_options(&self, image: DynamicImage, options: GraphProcessOptions) -> Option<GraphPreviewOutput> {
        let total_start = Instant::now();
        let call_idx = self.process_calls.fetch_add(1, Ordering::Relaxed);
        let requested_sample_ports = self.active_requested_sample_ports();
        self.apply_host_output_port_filter(options, &requested_sample_ports);
        if call_idx < 3 {
            tracing::debug!(call_idx, "daedalus graph: processing frame");
        }
        if self.disabled.load(Ordering::Relaxed) {
            let disabled_since = self.disabled_since_ms.load(Ordering::Relaxed);
            if call_idx < 3 || call_idx.is_multiple_of(120) {
                tracing::warn!(call_idx, disabled_since, "graph disabled after repeated errors; emitting error frame");
            }
            let detail = self.last_error_detail.read().ok().map(|guard| guard.trim().to_string()).filter(|text| !text.is_empty());
            self.image_working_set.record(dynamic_image_size_bytes(&image), 0, 0);
            return Some(GraphPreviewOutput::Image(error_frame_like(&image, "GRAPH DISABLED", detail.as_deref())));
        }
        if !self.dedicated_executor && self.rebuild_requested.swap(false, Ordering::Relaxed) {
            match self.rebuild_shared_executor() {
                Ok(()) => tracing::debug!(call_idx, "daedalus graph: rebuilt shared executor after failure"),
                Err(err) => {
                    self.rebuild_requested.store(true, Ordering::Relaxed);
                    tracing::warn!(call_idx, error = %err, "daedalus graph: failed to rebuild executor");
                }
            }
        }
        let gpu_plan_active = self.gpu.is_some() && plan_uses_gpu(self.plan.as_ref());
        let input_image_bytes = dynamic_image_size_bytes(&image);
        let input_dims = (image.width(), image.height());

        let push_host_inputs = |image: DynamicImage| -> Option<()> {
            let preview_only_port_reads = options.preview_only && requested_sample_ports.is_empty();
            for alias in &self.output_hosts {
                let Some(output_host) = self.host_mgr.handle(alias) else {
                    continue;
                };
                if preview_only_port_reads {
                    for port in &self.preview_ports {
                        let _ = output_host.clear(port);
                    }
                } else {
                    for port in output_host.incoming_port_names() {
                        let _ = output_host.clear(&port);
                    }
                }
            }

            let input_host = self.host_mgr.handle(&self.input_host_alias)?;
            let pushed = if gpu_plan_active {
                DaedalusEdgePayload::Data(DataCell::from_cpu::<DynamicImage>(image))
            } else {
                // CPU-only graphs still enter through the host bridge via `DataCell` so typed
                // image decoding stays on the same path as GPU-capable graphs.
                DaedalusEdgePayload::Data(DataCell::from_cpu::<DynamicImage>(image))
            };
            let correlation_id = input_host.push(&self.input_port, pushed, None);
            if call_idx < 3 {
                tracing::debug!(call_idx, correlation_id, port = %self.input_port, "daedalus graph: pushed input");
            }
            let calibration_port = self.calibration_port.clone().or_else(|| input_host.outgoing_ports().find(|p| p.eq_ignore_ascii_case("calibration")).map(|p| p.to_string()));

            if let Some(port) = calibration_port.as_deref() {
                let payload = self.calibration_payload.read().ok().map(|guard| guard.clone()).unwrap_or_else(|| calibration_to_daedalus_value(None));
                let pushed = DaedalusEdgePayload::Value(payload);
                let _ = input_host.push(port, pushed, Some(correlation_id));
            }
            if let Ok(guard) = self.input_values.read() {
                let auto_roi_values = if manual_roi_override_active(&guard) {
                    None
                } else {
                    self.auto_target_roi
                        .lock()
                        .ok()
                        .and_then(|state| state.rect.or_else(|| (!state.ever_detected).then(|| bootstrap_auto_target_roi_rect(input_dims, &guard)).flatten()))
                        .map(|rect| [("roi_x", rect.x), ("roi_y", rect.y), ("roi_w", rect.w), ("roi_h", rect.h)])
                };
                let auto_max_quads = if auto_roi_values.is_some() && self.declared_input_ports_lc.contains("max_quads") && !guard.contains_key("max_quads") { Some(4i64) } else { None };
                for (port, value) in guard.iter() {
                    if port.eq_ignore_ascii_case(&self.input_port) {
                        continue;
                    }
                    if let Some(cal_port) = calibration_port.as_deref() {
                        if port.eq_ignore_ascii_case(cal_port) {
                            continue;
                        }
                    }
                    if auto_roi_values.is_some() && is_roi_port(port) {
                        continue;
                    }
                    let pushed = DaedalusEdgePayload::Value(value.clone());
                    let _ = input_host.push(port, pushed, Some(correlation_id));
                }
                if let Some(auto_roi_values) = auto_roi_values {
                    for (port, value) in auto_roi_values {
                        let _ = input_host.push(port, DaedalusEdgePayload::Value(DaedalusValue::Int(value)), Some(correlation_id));
                    }
                }
                if let Some(max_quads) = auto_max_quads {
                    let _ = input_host.push("max_quads", DaedalusEdgePayload::Value(DaedalusValue::Int(max_quads)), Some(correlation_id));
                }
            }
            Some(())
        };

        let perf_guard = if self.perf_enabled.load(Ordering::Relaxed) {
            match perf::PerfCounterGuard::start() {
                Ok(guard) => Some(guard),
                Err(err) => {
                    self.perf_enabled.store(false, Ordering::Relaxed);
                    tracing::warn!(error = %err, "perf counters unavailable; disabling");
                    None
                }
            }
        } else {
            None
        };

        if self.pprof_pending.load(Ordering::Relaxed) {
            if let Ok(mut guard_slot) = self.pprof_guard.lock() {
                if guard_slot.is_none() {
                    let path = flamegraph::build_flamegraph_path(call_idx);
                    match flamegraph::FlamegraphGuard::start(path) {
                        Ok(guard) => {
                            *guard_slot = Some(guard);
                        }
                        Err(err) => {
                            self.pprof_pending.store(false, Ordering::Relaxed);
                            tracing::warn!(error = %err, "flamegraph capture failed");
                        }
                    }
                }
            }
        }

        let mut lock_duration = Duration::default();
        let run_result: Result<(DaedalusExecutionTelemetry, Duration), String> = if self.dedicated_executor {
            push_host_inputs(image)?;
            let host_outputs_in_graph = policy::host_outputs_in_graph_enabled(Some(self.plan.as_ref()), gpu_plan_active);
            let active_nodes = self.active_nodes_for_process_options(options);
            let mut exec = DaedalusOwnedExecutor::new(self.plan.clone(), self.handlers.clone_arc())
                .with_host_bridges(self.host_mgr.clone())
                .with_const_coercers(self.const_coercers.clone())
                .with_output_movers(self.output_movers.clone())
                .with_fail_fast(false)
                .with_metrics_level(self.run_metrics_level)
                .with_host_outputs_in_graph(host_outputs_in_graph)
                .with_active_nodes_mask(active_nodes);
            if let Some(handle) = self.gpu.clone() {
                exec = exec.with_gpu(handle);
            }
            if let Some(size) = self.pool_size {
                exec = exec.with_pool_size(Some(size));
            }
            let run_start = Instant::now();
            let result = catch_unwind(AssertUnwindSafe(|| match self.run_mode {
                RuntimeMode::Serial => exec.run_in_place().map(|telemetry| (telemetry, run_start.elapsed())),
                _ => exec.run_parallel_in_place().map(|telemetry| (telemetry, run_start.elapsed())),
            }));
            match result {
                Ok(result) => result.map_err(|err| format!("{err:?}")),
                Err(panic) => Err(format!("panic: {}", format_panic_message(&panic))),
            }
        } else {
            let lock_start = Instant::now();
            let mut exec = match self.busy_behavior {
                ExecutorBusyBehavior::Drop => match self.executor.try_lock() {
                    Ok(lock) => Some(lock),
                    Err(TryLockError::WouldBlock) => None,
                    Err(TryLockError::Poisoned(err)) => Some(err.into_inner()),
                },
                ExecutorBusyBehavior::Block => {
                    if let Some(timeout) = self.busy_timeout {
                        let deadline = Instant::now() + timeout;
                        loop {
                            match self.executor.try_lock() {
                                Ok(lock) => break Some(lock),
                                Err(TryLockError::WouldBlock) => {}
                                Err(TryLockError::Poisoned(err)) => break Some(err.into_inner()),
                            }
                            if Instant::now() >= deadline {
                                break None;
                            }
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    } else {
                        // If the executor previously panicked while holding the lock, recover the
                        // inner executor and keep the stream alive (otherwise the preview freezes).
                        Some(self.executor.lock().unwrap_or_else(PoisonError::into_inner))
                    }
                }
            };

            lock_duration = lock_start.elapsed();
            let lock_ms = lock_duration.as_secs_f64() * 1000.0;
            histogram!("helios.stream.executor_lock_ms").record(lock_ms);

            let exec = exec.as_deref_mut()?;
            push_host_inputs(image)?;
            exec.set_active_nodes_mask(self.active_nodes_for_process_options(options));

            let run_start = Instant::now();
            let result = catch_unwind(AssertUnwindSafe(|| match self.run_mode {
                RuntimeMode::Serial => exec.run_in_place().map(|telemetry| (telemetry, run_start.elapsed())),
                _ => exec.run_parallel_in_place().map(|telemetry| (telemetry, run_start.elapsed())),
            }));
            match result {
                Ok(result) => result.map_err(|err| format!("{err:?}")),
                Err(panic) => Err(format!("panic: {}", format_panic_message(&panic))),
            }
        };
        let (telemetry, run_duration) = match run_result {
            Ok(result) => result,
            Err(err_text) => {
                if call_idx < 3 || call_idx.is_multiple_of(120) {
                    tracing::warn!(call_idx, error = %err_text, "graph execution failed; emitting error frame");
                }
                let node_type = extract_node_type(&err_text);
                if let Ok(mut metrics) = self.metrics.lock() {
                    metrics.record_error(node_type.as_deref(), err_text.clone());
                }
                if !self.dedicated_executor {
                    self.rebuild_requested.store(true, Ordering::Relaxed);
                }
                let failures = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
                if failures >= GRAPH_ERROR_DISABLE_THRESHOLD {
                    self.failure_count.store(0, Ordering::Relaxed);
                    self.disabled.store(true, Ordering::Relaxed);
                    self.disabled_since_ms.store(now_ms(), Ordering::Relaxed);
                    tracing::warn!(call_idx, failures, "graph disabled after repeated errors; awaiting stream/graph update");
                }
                let detail = format_error_detail(&err_text, node_type.as_deref());
                if let Ok(mut guard) = self.last_error_detail.write() {
                    *guard = detail.clone();
                }
                self.image_working_set.record(input_image_bytes, 0, 0);
                return Some(GraphPreviewOutput::Image(error_frame(input_dims, "GRAPH ERROR", Some(&detail))));
            }
        };
        let perf_sample = perf_guard.and_then(|guard| guard.finish().ok());
        let flamegraph_capture = if self.pprof_pending.load(Ordering::Relaxed) {
            let mut capture = None;
            let now_wall_ms = now_ms();
            if let Ok(mut guard_slot) = self.pprof_guard.lock() {
                if guard_slot.is_some() {
                    let until_ms = self.pprof_until_ms.load(Ordering::Relaxed);
                    if until_ms > 0 {
                        if now_wall_ms >= until_ms {
                            if let Some(guard) = guard_slot.take() {
                                capture = guard.finish().ok();
                            }
                            self.pprof_pending.store(false, Ordering::Relaxed);
                            self.pprof_until_ms.store(0, Ordering::Relaxed);
                            self.pprof_remaining.store(0, Ordering::Relaxed);
                        }
                    } else {
                        let remaining = self.pprof_remaining.load(Ordering::Relaxed);
                        if remaining > 0 {
                            let next = remaining.saturating_sub(1);
                            self.pprof_remaining.store(next, Ordering::Relaxed);
                            if next == 0 {
                                if let Some(guard) = guard_slot.take() {
                                    capture = guard.finish().ok();
                                }
                                self.pprof_pending.store(false, Ordering::Relaxed);
                                self.pprof_until_ms.store(0, Ordering::Relaxed);
                            }
                        }
                    }
                }
            }
            capture
        } else {
            None
        };
        self.failure_count.store(0, Ordering::Relaxed);
        if call_idx < 3 {
            tracing::debug!(call_idx, warnings = telemetry.warnings.len(), nodes = telemetry.node_metrics.len(), "daedalus graph: ran");
        }

        let output_materialization_start = Instant::now();
        let mut preview_output: Option<GraphPreviewOutput> = None;
        let mut preview_key: Option<String> = None;
        let mut image_updates: Vec<(String, DynamicImage)> = Vec::new();
        let mut value_updates: Vec<(String, DaedalusValue)> = Vec::new();
        let mut typed_updates: Vec<(String, TypedHostOutputSample)> = Vec::new();
        let mut popped_outputs = 0usize;
        let image_output_requested = options.require_image_output;
        let preview_only_port_reads = options.preview_only && requested_sample_ports.is_empty();

        for alias in &self.output_hosts {
            let Some(output_host) = self.host_mgr.handle(alias) else { continue };
            if call_idx < 3 {
                tracing::debug!(call_idx, host = %alias, ports = ?output_host.incoming_port_names(), "daedalus graph: output host ports");
            }
            let port_names: Vec<String> = if preview_only_port_reads {
                self.preview_ports.iter().chain(self.preview_aux_ports.iter()).cloned().collect::<BTreeSet<_>>().into_iter().collect()
            } else {
                output_host.incoming_port_names()
            };
            for port_name_owned in port_names {
                let mut ports = output_host.iter_ports(std::slice::from_ref(&port_name_owned));
                let Some(port) = ports.next() else { continue };
                let port_name = port.name();
                let port_lc = port_name.to_ascii_lowercase();
                if !self.host_output_ports_lc.contains(&port_lc) {
                    // The graph JSON contract is authoritative. If the runtime exposes extra
                    // ports (e.g. due to stale persisted graphs or dynamic nodes), drain+ignore.
                    let _ = output_host.clear(port_name);
                    continue;
                }
                let wants_preview = image_output_requested && self.preview_ports_lc.contains(&port_lc);
                let wants_retained_sample = requested_sample_ports.contains(&port_lc);
                if preview_only_port_reads && !wants_preview {
                    continue;
                }
                let wants_image_sample = wants_preview || wants_retained_sample;
                let direct_preview = wants_preview && !wants_retained_sample;
                let port_type = port.resolved_type();
                let prefers_grayscale_preview = direct_preview && preview_port_accepts_grayscale_input(&port_lc, &self.host_output_port_types);
                let is_image_type = port_type.map(is_image_payload).unwrap_or(false);
                let typed_image = is_image_type;
                if policy::host_output_debug_enabled() && (call_idx < 3 || call_idx.is_multiple_of(120)) {
                    tracing::info!(
                        target: "helios_engine::graph",
                        port = %port_name,
                        wants_preview,
                        wants_retained_sample,
                        typed_image,
                        resolved_type = ?port_type,
                        "host output port state"
                    );
                }
                if typed_image && !wants_image_sample {
                    popped_outputs += output_host.clear(port_name);
                    continue;
                }
                // Keep image samples only for the active preview/output path. Additional image
                // outputs can be surprisingly expensive because they force CPU materialization and
                // then stay resident in `image_samples`.
                //
                // Unresolved ports are common while type inference is catching up, but they should
                // not cause us to eagerly materialize large image payloads unless the caller
                // explicitly asked for that image.
                let unresolved_type = port_type.is_none();
                if typed_image || (unresolved_type && wants_image_sample) {
                    let mut image_popped = false;

                    if prefers_grayscale_preview {
                        match port.try_pop::<Compute<GrayImage>>() {
                            Ok(Some((_corr, payload))) => match payload {
                                Compute::Cpu(gray) => {
                                    popped_outputs += 1;
                                    if preview_key.is_none() && wants_preview {
                                        preview_key = Some(port_lc.clone());
                                    }
                                    preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                    image_popped = true;
                                    if call_idx < 3 && wants_preview {
                                        tracing::debug!(call_idx, port = %port_name, "daedalus graph: pulled grayscale preview output");
                                    }
                                }
                                Compute::Gpu(handle) => {
                                    if let Some(gpu) = self.gpu.as_ref() {
                                        match <GrayImage as daedalus::gpu::DeviceBridge>::download(&handle, gpu) {
                                            Ok(gray) => {
                                                popped_outputs += 1;
                                                if preview_key.is_none() && wants_preview {
                                                    preview_key = Some(port_lc.clone());
                                                }
                                                preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                                image_popped = true;
                                            }
                                            Err(err) => {
                                                if wants_preview || policy::host_output_debug_enabled() {
                                                    tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output GPU gray decode failed");
                                                }
                                            }
                                        }
                                    }
                                }
                            },
                            Ok(None) => {}
                            Err(_err) => {}
                        }

                        if !image_popped {
                            if let Some((_corr, gray)) = port.try_pop_any_arc::<GrayImage>() {
                                popped_outputs += 1;
                                if preview_key.is_none() && wants_preview {
                                    preview_key = Some(port_lc.clone());
                                }
                                preview_output = Some(GraphPreviewOutput::Gray(gray));
                                image_popped = true;
                            }
                        }

                        if !image_popped {
                            if let Some((_corr, gray)) = port.try_pop_any::<GrayImage>() {
                                popped_outputs += 1;
                                if preview_key.is_none() && wants_preview {
                                    preview_key = Some(port_lc.clone());
                                }
                                preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                image_popped = true;
                            }
                        }
                    }

                    if !image_popped {
                        match port.try_pop::<DynamicImage>() {
                            Ok(Some((_corr, img))) => {
                                popped_outputs += 1;
                                if preview_key.is_none() && wants_preview {
                                    preview_key = Some(port_lc.clone());
                                }
                                if direct_preview && preview_output.is_none() {
                                    preview_output = Some(normalize_preview_output_for_port(GraphPreviewOutput::Image(img), &port_lc, &self.host_output_port_types));
                                } else {
                                    image_updates.push((port_lc.clone(), img));
                                }
                                image_popped = true;
                                if call_idx < 3 && wants_preview {
                                    tracing::debug!(call_idx, port = %port_name, "daedalus graph: pulled preview output");
                                }
                            }
                            Ok(None) => {}
                            Err(_err) => {
                                // Many CV nodes in lib-cv use `Compute<DynamicImage>` so they can
                                // run under GPU/CPU affinity without forcing node authors to
                                // manually handle transfers. The host output bridge should be
                                // able to decode those payloads too.
                                match port.try_pop::<Compute<DynamicImage>>() {
                                    Ok(Some((_corr, payload))) => match payload {
                                        Compute::Cpu(img) => {
                                            popped_outputs += 1;
                                            if preview_key.is_none() && wants_preview {
                                                preview_key = Some(port_lc.clone());
                                            }
                                            if direct_preview && preview_output.is_none() {
                                                preview_output = Some(normalize_preview_output_for_port(GraphPreviewOutput::Image(img), &port_lc, &self.host_output_port_types));
                                            } else {
                                                image_updates.push((port_lc.clone(), img));
                                            }
                                            image_popped = true;
                                        }
                                        Compute::Gpu(handle) => {
                                            if let Some(gpu) = self.gpu.as_ref() {
                                                match <DynamicImage as daedalus::gpu::DeviceBridge>::download(&handle, gpu) {
                                                    Ok(img) => {
                                                        popped_outputs += 1;
                                                        if preview_key.is_none() && wants_preview {
                                                            preview_key = Some(port_lc.clone());
                                                        }
                                                        if direct_preview && preview_output.is_none() {
                                                            preview_output = Some(normalize_preview_output_for_port(GraphPreviewOutput::Image(img), &port_lc, &self.host_output_port_types));
                                                        } else {
                                                            image_updates.push((port_lc.clone(), img));
                                                        }
                                                        image_popped = true;
                                                    }
                                                    Err(err) => {
                                                        if wants_preview || policy::host_output_debug_enabled() {
                                                            tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output GPU image download failed");
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    },
                                    Ok(None) => {}
                                    Err(_err) => {}
                                }

                                if !image_popped {
                                    match port.try_pop::<Compute<GrayImage>>() {
                                        Ok(Some((_corr, payload))) => match payload {
                                            Compute::Cpu(gray) => {
                                                popped_outputs += 1;
                                                if preview_key.is_none() && wants_preview {
                                                    preview_key = Some(port_lc.clone());
                                                }
                                                if direct_preview && preview_output.is_none() {
                                                    preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                                } else {
                                                    image_updates.push((port_lc.clone(), DynamicImage::ImageLuma8(gray)));
                                                }
                                                image_popped = true;
                                                if call_idx < 3 && wants_preview {
                                                    tracing::debug!(call_idx, port = %port_name, "daedalus graph: pulled preview output");
                                                }
                                            }
                                            Compute::Gpu(handle) => {
                                                if let Some(gpu) = self.gpu.as_ref() {
                                                    match <GrayImage as daedalus::gpu::DeviceBridge>::download(&handle, gpu) {
                                                        Ok(gray) => {
                                                            popped_outputs += 1;
                                                            if preview_key.is_none() && wants_preview {
                                                                preview_key = Some(port_lc.clone());
                                                            }
                                                            if direct_preview && preview_output.is_none() {
                                                                preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                                            } else {
                                                                image_updates.push((port_lc.clone(), DynamicImage::ImageLuma8(gray)));
                                                            }
                                                            image_popped = true;
                                                        }
                                                        Err(err) => {
                                                            if wants_preview || policy::host_output_debug_enabled() {
                                                                tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output GPU gray decode failed");
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        Ok(None) => {}
                                        Err(_err) => {}
                                    }
                                }

                                if !image_popped {
                                    if let Some((_corr, gray)) = port.try_pop_any::<GrayImage>() {
                                        popped_outputs += 1;
                                        if preview_key.is_none() && wants_preview {
                                            preview_key = Some(port_lc.clone());
                                        }
                                        if direct_preview && preview_output.is_none() {
                                            preview_output = Some(GraphPreviewOutput::Gray(Arc::new(gray)));
                                        } else {
                                            image_updates.push((port_lc.clone(), DynamicImage::ImageLuma8(gray)));
                                        }
                                        image_popped = true;
                                    }
                                }

                                if !image_popped {
                                    if let Some((_corr, gray)) = port.try_pop_any_arc::<GrayImage>() {
                                        popped_outputs += 1;
                                        if preview_key.is_none() && wants_preview {
                                            preview_key = Some(port_lc.clone());
                                        }
                                        if direct_preview && preview_output.is_none() {
                                            preview_output = Some(GraphPreviewOutput::Gray(gray));
                                        } else {
                                            image_updates.push((port_lc.clone(), DynamicImage::ImageLuma8(Arc::unwrap_or_clone(gray))));
                                        }
                                        image_popped = true;
                                    }
                                }
                            }
                        }
                    }

                    if image_popped {
                        continue;
                    }
                    if typed_image {
                        // Typed image outputs can be empty on this tick; no value decode fallback.
                        continue;
                    }
                }

                let retain_structured_output = should_retain_structured_output_port(&port_lc, &requested_sample_ports, self.auto_target_roi_source_port.as_deref(), !self.preview_ports.is_empty());
                if !retain_structured_output {
                    popped_outputs += output_host.clear(port_name);
                    continue;
                }

                // Structured-only graphs still retain their latest outputs because downstream
                // readers pull them synchronously after each process call. Stream graphs with
                // preview/image outputs only keep structured ports that are explicitly requested
                // or needed for auto-ROI tracking.

                if port_type.is_some_and(is_aruco_detections_payload) {
                    if let Some(raw) = port.try_pop_raw() {
                        if let Some(detections) = decode_runtime_value_as_aruco_detections(&raw.inner) {
                            popped_outputs += 1;
                            let port_name = port_lc.clone();
                            if call_idx < 3 {
                                tracing::debug!(
                                    call_idx,
                                    port = %port_name,
                                    len = detections.len(),
                                    "daedalus graph: captured typed detection output"
                                );
                            }
                            typed_updates.push((port_name, TypedHostOutputSample::ArucoDetections(detections)));
                            continue;
                        }
                        port.restore_raw(raw);
                    }
                }

                match port.try_pop::<daedalus::data::model::Value>() {
                    Ok(Some((_corr, value))) => {
                        popped_outputs += 1;
                        let port_name = port_lc.clone();
                        value_updates.push((port_name, value));
                        continue;
                    }
                    Ok(None) => {
                        if let Some(raw) = port.try_pop_raw() {
                            if let Some(value) = decode_runtime_value_fallback(&raw.inner, port_type) {
                                popped_outputs += 1;
                                let port_name = port_lc.clone();
                                value_updates.push((port_name, value));
                                continue;
                            }
                            port.restore_raw(raw);
                        }
                    }
                    Err(err) => {
                        if let Some(raw) = port.try_pop_raw() {
                            if let Some(value) = decode_runtime_value_fallback(&raw.inner, port_type) {
                                popped_outputs += 1;
                                let port_name = port_lc.clone();
                                value_updates.push((port_name, value));
                                continue;
                            }
                            port.restore_raw(raw);
                        }

                        if policy::host_output_debug_enabled() {
                            // Many ports are neither image nor value-like; ignore unless debugging.
                            tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output value decode failed");
                        }
                    }
                }

                if unresolved_type && !wants_image_sample {
                    // If the port type is still unresolved and we did not recognize a structured
                    // value, drain anything left so image outputs do not accumulate or get
                    // repeatedly materialized on later ticks.
                    popped_outputs += output_host.clear(port_name);
                }
            }
        }

        if call_idx < 3 {
            tracing::debug!(call_idx, popped_outputs, "daedalus graph: output host pop count");
        }

        if !value_updates.is_empty() {
            if let Ok(mut guard) = self.value_samples.lock() {
                for (port, value) in value_updates {
                    guard.insert(port, value);
                }
            }
        }
        if !typed_updates.is_empty() {
            if let Some(source_port) = self.auto_target_roi_source_port.as_deref() {
                if let Some((_port, TypedHostOutputSample::ArucoDetections(detections))) = typed_updates.iter().find(|(port, _)| port == source_port) {
                    if let Ok(mut guard) = self.auto_target_roi.lock() {
                        let _ = update_auto_target_roi_state(&mut guard, input_dims, detections.as_ref().as_slice());
                    }
                }
            }
            if let Ok(mut guard) = self.typed_samples.lock() {
                for (port, value) in typed_updates {
                    guard.insert(port, value);
                }
            }
        }
        if !image_updates.is_empty() {
            // Select the preview image locally first so we can still render a frame even if the
            // sample cache lock is poisoned/unavailable.
            if preview_output.is_none() {
                if let Some(key) = preview_key.as_deref() {
                    if let Some(idx) = image_updates.iter().position(|(port, _)| port == key) {
                        let (_port, img) = image_updates.swap_remove(idx);
                        preview_output = Some(normalize_preview_output_for_port(GraphPreviewOutput::Image(img), key, &self.host_output_port_types));
                    }
                }
            }
            let host_output_image_bytes: u64 = image_updates.iter().map(|(_, image)| dynamic_image_size_bytes(image)).sum();
            let preview_image_bytes = preview_output.as_ref().map(GraphPreviewOutput::size_bytes).unwrap_or(0);
            self.image_working_set.record(input_image_bytes, host_output_image_bytes, preview_image_bytes);
            if let Ok(mut guard) = self.image_samples.lock() {
                for (port, value) in image_updates {
                    guard.insert(port, value);
                }
            }
        } else {
            let preview_image_bytes = preview_output.as_ref().map(GraphPreviewOutput::size_bytes).unwrap_or(0);
            self.image_working_set.record(input_image_bytes, 0, preview_image_bytes);
        }
        self.prune_unrequested_output_samples(&requested_sample_ports);

        let output_materialization_duration = output_materialization_start.elapsed();
        let wrapper_duration = total_start.elapsed().saturating_sub(run_duration);
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.record_graph_duration(run_duration);
            metrics.record_wrapper_duration(wrapper_duration);
            metrics.record_lock_duration(lock_duration);
            metrics.record_output_materialization_duration(output_materialization_duration);
            metrics.record_telemetry(&telemetry);
            if let Some(sample) = perf_sample {
                metrics.record_perf_sample(sample);
            }
            if let Some(capture) = flamegraph_capture {
                metrics.record_flamegraph(capture);
            }
        }

        if let Some(img) = preview_output {
            self.maybe_trim_background_graph_allocators(options);
            return Some(img);
        }
        if !image_output_requested {
            self.maybe_trim_background_graph_allocators(options);
            return None;
        }
        // If no output port produced a frame, report it and keep the last good preview frame.
        if call_idx < 3 || call_idx.is_multiple_of(120) {
            tracing::warn!(call_idx, "graph produced no output; falling back to last preview");
        }
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.record_warning(format!("graph output missing: selected ports {:?} produced no frames", self.preview_ports));
        }
        self.maybe_trim_background_graph_allocators(options);
        None
    }

    fn pipeline_metrics(&self) -> Option<PipelineGraphMetrics> {
        self.metrics.lock().ok().map(|metrics| {
            let mut snapshot = metrics.snapshot();
            snapshot.sample_cache = sample_cache_metrics(&self.image_samples, &self.value_samples, &self.typed_samples);
            snapshot.image_working_set = self.image_working_set.snapshot();
            annotate_retained_output_metrics(&mut snapshot, &self.host_output_port_owners);
            snapshot
        })
    }

    fn host_output_ports(&self) -> Option<Vec<String>> {
        Some(self.host_output_ports.clone())
    }

    fn request_output_sample(&self, port: &str) {
        self.request_output_sample_retention(port);
    }

    fn has_output_sample_demand(&self) -> bool {
        !self.active_requested_sample_ports().is_empty()
    }

    fn has_image_output(&self) -> bool {
        !self.preview_ports.is_empty()
    }

    fn prefers_grayscale_input(&self) -> bool {
        self.prefers_grayscale_input
    }

    fn host_output_port_types(&self) -> Option<BTreeMap<String, DaedalusTypeExpr>> {
        Some(self.host_output_port_types.clone())
    }

    fn sample_json_output(&self, port: &str) -> Option<Value> {
        let key = port.to_ascii_lowercase();
        if let Some(value) = self.value_samples.lock().ok()?.get(&key).cloned() {
            return daedalus_value_to_json(&value);
        }
        self.typed_samples.lock().ok()?.get(&key).and_then(TypedHostOutputSample::to_json)
    }

    fn sample_value_output(&self, port: &str) -> Option<DaedalusValue> {
        let key = port.to_ascii_lowercase();
        if let Some(value) = self.value_samples.lock().ok()?.get(&key).cloned() {
            return Some(value);
        }
        self.typed_samples.lock().ok()?.get(&key).and_then(TypedHostOutputSample::to_daedalus_value)
    }

    fn sample_image_output(&self, port: &str) -> Option<DynamicImage> {
        let key = port.to_ascii_lowercase();
        self.image_samples.lock().ok()?.get(&key).cloned()
    }

    fn disabled_state(&self) -> GraphDisabledState {
        let disabled = self.disabled.load(Ordering::Relaxed);
        if !disabled {
            return GraphDisabledState::default();
        }
        let disabled_since_ms = self.disabled_since_ms.load(Ordering::Relaxed);
        let reason = self.last_error_detail.read().ok().map(|guard| guard.trim().to_string()).filter(|text| !text.is_empty());
        GraphDisabledState { disabled, disabled_since_ms: if disabled_since_ms == 0 { None } else { Some(disabled_since_ms) }, disabled_reason: reason }
    }

    fn clear_disabled(&self) {
        self.disabled.store(false, Ordering::Relaxed);
        self.disabled_since_ms.store(0, Ordering::Relaxed);
        self.failure_count.store(0, Ordering::Relaxed);
    }

    fn set_perf_enabled(&self, _pipeline_id: Option<uuid::Uuid>, enabled: bool) {
        // If the feature isn't compiled in, keep it off.
        if !cfg!(all(feature = "perf-counters", target_os = "linux")) {
            self.perf_enabled.store(false, Ordering::Relaxed);
            return;
        }
        self.perf_enabled.store(enabled, Ordering::Relaxed);
    }

    fn reset_pipeline_metrics(&self, _pipeline_id: Option<uuid::Uuid>) {
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.reset();
        }
    }

    fn release_idle_retention(&self) {
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.release_idle_retention();
        }
        self.image_working_set.clear_current();
        if let Ok(mut guard) = self.image_samples.lock() {
            guard.clear();
        }
        if let Ok(mut guard) = self.value_samples.lock() {
            guard.clear();
        }
        if let Ok(mut guard) = self.typed_samples.lock() {
            guard.clear();
        }
        if let Ok(mut guard) = self.requested_sample_ports.lock() {
            guard.clear();
        }
        if let Ok(exec) = self.executor.lock() {
            let _ = exec.on_idle();
        }
        for alias in &self.output_hosts {
            let Some(output_host) = self.host_mgr.handle(alias) else {
                continue;
            };
            for port in output_host.incoming_ports() {
                let _ = output_host.clear(port.name());
            }
        }
    }

    fn capture_flamegraph(&self, _pipeline_id: Option<uuid::Uuid>, duration_ms: u64) -> Result<(), String> {
        if duration_ms == 0 {
            return Err("duration_ms must be > 0".into());
        }
        if !cfg!(feature = "pprof") {
            return Err("pprof feature not enabled".into());
        }

        // Reject concurrent captures (keeps metrics predictable + avoids racing guard).
        if self.pprof_pending.swap(true, Ordering::Relaxed) {
            return Err("flamegraph capture already in progress".into());
        }
        // Clear frame-based mode and use wall-clock duration.
        self.pprof_remaining.store(0, Ordering::Relaxed);
        self.pprof_until_ms.store(now_ms().saturating_add(duration_ms), Ordering::Relaxed);
        Ok(())
    }
}

impl DaedalusGraphExecutor {
    fn apply_host_output_port_filter(&self, options: GraphProcessOptions, requested_sample_ports: &BTreeSet<String>) {
        let filter =
            if options.preview_only && requested_sample_ports.is_empty() { Some(self.preview_ports_lc.iter().chain(self.preview_aux_ports_lc.iter()).cloned().collect::<BTreeSet<_>>()) } else { None };

        for alias in &self.output_hosts {
            self.host_mgr.set_outbound_port_filter(alias.clone(), filter.clone());
        }
    }

    fn active_nodes_for_process_options(&self, options: GraphProcessOptions) -> Option<Arc<Vec<bool>>> {
        if options.preview_only {
            return self.active_nodes_preview_only.clone().or_else(|| self.active_nodes_with_image.clone());
        }
        if options.require_image_output {
            return self.active_nodes_with_image.clone();
        }
        self.active_nodes_without_image.clone().or_else(|| self.active_nodes_with_image.clone())
    }

    fn maybe_trim_background_graph_allocators(&self, options: GraphProcessOptions) {
        let interval_ms = if options.require_image_output { policy::active_graph_trim_interval_ms() } else { policy::background_graph_trim_interval_ms() };
        if interval_ms == 0 {
            return;
        }
        let now = now_ms();
        let last = self.last_background_trim_ms.load(Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < interval_ms {
            return;
        }
        self.last_background_trim_ms.store(now, Ordering::Relaxed);
        styx::codec::decoder::clear_packed_frame_pools_all_threads();
        lib_cv::compact_runtime_scratch_after_frame();
    }

    fn normalize_host_output_port_key(&self, port: &str) -> Option<String> {
        let key = port.trim().to_ascii_lowercase();
        if key.is_empty() || !self.host_output_ports_lc.contains(&key) {
            return None;
        }
        Some(key)
    }

    fn request_output_sample_retention(&self, port: &str) {
        let Some(key) = self.normalize_host_output_port_key(port) else {
            return;
        };
        if let Ok(mut guard) = self.requested_sample_ports.lock() {
            let requested_at_ms = now_ms();
            if policy::host_output_debug_enabled() {
                tracing::info!(
                    target: "helios_engine::graph",
                    port = %key,
                    requested_at_ms,
                    "host output sample retention requested"
                );
            }
            guard.insert(key, requested_at_ms);
        }
    }

    fn active_requested_sample_ports(&self) -> BTreeSet<String> {
        let mut active = BTreeSet::new();
        let now = now_ms();
        let ttl_ms = policy::host_output_sample_ttl_ms();
        if let Ok(mut guard) = self.requested_sample_ports.lock() {
            guard.retain(|port, requested_at_ms| {
                let keep = now.saturating_sub(*requested_at_ms) <= ttl_ms;
                if keep {
                    active.insert(port.clone());
                }
                keep
            });
        }
        active
    }

    fn prune_unrequested_output_samples(&self, requested_ports: &BTreeSet<String>) {
        // Structured outputs are retained as rolling last-sample state. Only image outputs are
        // aggressively pruned because they materially impact memory use.
        if let Ok(mut guard) = self.image_samples.lock() {
            guard.retain(|port, _| requested_ports.contains(port));
        }
    }
}

fn daedalus_value_to_json(value: &daedalus::data::model::Value) -> Option<Value> {
    match value {
        daedalus::data::model::Value::String(s) => serde_json::from_str::<Value>(s).ok().or_else(|| Some(Value::String(s.to_string()))),
        daedalus::data::model::Value::Bytes(bytes) => serde_json::from_slice::<Value>(bytes.as_ref()).ok().or_else(|| Some(Value::String(String::from_utf8_lossy(bytes).to_string()))),
        _ => daedalus_value_to_plain_json(value),
    }
}

fn daedalus_value_to_plain_json(value: &daedalus::data::model::Value) -> Option<Value> {
    use daedalus::data::model::Value as RawValue;
    match value {
        RawValue::Unit => Some(Value::Null),
        RawValue::Bool(b) => Some(Value::Bool(*b)),
        RawValue::Int(i) => Some(Value::Number((*i).into())),
        RawValue::Float(f) => serde_json::Number::from_f64(*f).map(Value::Number),
        RawValue::String(s) => Some(Value::String(s.to_string())),
        RawValue::Bytes(bytes) => Some(Value::String(String::from_utf8_lossy(bytes).to_string())),
        RawValue::List(items) | RawValue::Tuple(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(daedalus_value_to_plain_json(item)?);
            }
            Some(Value::Array(out))
        }
        RawValue::Struct(fields) => {
            let mut out = serde_json::Map::new();
            for field in fields {
                out.insert(field.name.clone(), daedalus_value_to_plain_json(&field.value)?);
            }
            Some(Value::Object(out))
        }
        RawValue::Enum(ev) => {
            let mut out = serde_json::Map::new();
            out.insert("name".into(), Value::String(ev.name.clone()));
            if let Some(inner) = ev.value.as_deref() {
                out.insert("value".into(), daedalus_value_to_plain_json(inner)?);
            }
            Some(Value::Object(out))
        }
        RawValue::Map(entries) => {
            let mut obj = serde_json::Map::new();
            let mut arr = Vec::with_capacity(entries.len());
            let mut all_string_keys = true;
            for (key, value) in entries {
                let key_json = daedalus_value_to_plain_json(key)?;
                let value_json = daedalus_value_to_plain_json(value)?;
                if let Value::String(key_str) = key_json {
                    obj.insert(key_str, value_json);
                } else {
                    all_string_keys = false;
                    arr.push(Value::Array(vec![key_json, value_json]));
                }
            }
            if all_string_keys {
                Some(Value::Object(obj))
            } else {
                Some(Value::Array(arr))
            }
        }
    }
}

fn sample_cache_metrics(
    image_samples: &Mutex<BTreeMap<String, DynamicImage>>,
    value_samples: &Mutex<BTreeMap<String, DaedalusValue>>,
    typed_samples: &Mutex<BTreeMap<String, TypedHostOutputSample>>,
) -> Option<PipelineSampleCacheMetrics> {
    let image_ports = image_samples.lock().ok().map(|guard| guard.iter().map(|(port, image)| (port.clone(), dynamic_image_size_bytes(image))).collect::<BTreeMap<_, _>>())?;
    let mut value_ports = value_samples.lock().ok().map(|guard| guard.iter().map(|(port, value)| (port.clone(), daedalus_value_size_bytes(value))).collect::<BTreeMap<_, _>>())?;
    let typed_ports = typed_samples.lock().ok().map(|guard| guard.iter().map(|(port, sample)| (port.clone(), sample.size_bytes())).collect::<BTreeMap<_, _>>())?;
    for (port, bytes) in typed_ports {
        value_ports.entry(port).or_insert(bytes);
    }

    let metrics = PipelineSampleCacheMetrics {
        image_sample_count: image_ports.len() as u64,
        image_sample_bytes: image_ports.values().copied().sum(),
        json_sample_count: 0,
        json_sample_bytes: 0,
        value_sample_count: value_ports.len() as u64,
        value_sample_bytes: value_ports.values().copied().sum(),
        image_ports,
        json_ports: BTreeMap::new(),
        value_ports,
    };

    if metrics.image_sample_count == 0 && metrics.value_sample_count == 0 && metrics.image_sample_bytes == 0 && metrics.value_sample_bytes == 0 {
        None
    } else {
        Some(metrics)
    }
}

fn annotate_retained_output_metrics(snapshot: &mut PipelineGraphMetrics, owners: &BTreeMap<String, usize>) {
    let Some(sample_cache) = snapshot.sample_cache.as_ref() else {
        return;
    };

    let mut bytes_by_node: BTreeMap<usize, u64> = BTreeMap::new();
    let mut counts_by_node: BTreeMap<usize, u64> = BTreeMap::new();
    let mut ports_by_node: BTreeMap<usize, BTreeMap<String, u64>> = BTreeMap::new();

    let mut record_ports = |ports: &BTreeMap<String, u64>| {
        for (port, bytes) in ports {
            let Some(node_index) = owners.get(&port.to_ascii_lowercase()).copied() else {
                continue;
            };
            *bytes_by_node.entry(node_index).or_default() += *bytes;
            *counts_by_node.entry(node_index).or_default() += 1;
            *ports_by_node.entry(node_index).or_default().entry(port.clone()).or_default() += *bytes;
        }
    };

    record_ports(&sample_cache.image_ports);
    record_ports(&sample_cache.json_ports);
    record_ports(&sample_cache.value_ports);

    for node in snapshot.nodes.values_mut() {
        let Some(node_index) = node.node_index.map(|value| value as usize) else {
            continue;
        };
        let retained_bytes = bytes_by_node.get(&node_index).copied().unwrap_or(0);
        let retained_count = counts_by_node.get(&node_index).copied().unwrap_or(0);
        if retained_bytes == 0 && retained_count == 0 {
            continue;
        }
        node.retained_output_sample_bytes = retained_bytes;
        node.retained_output_sample_count = retained_count;
        node.retained_output_ports = ports_by_node.get(&node_index).cloned();
    }
}

fn dynamic_image_size_bytes(image: &DynamicImage) -> u64 {
    u64::from(image.width()).saturating_mul(u64::from(image.height())).saturating_mul(u64::from(image.color().bytes_per_pixel() as u32))
}

fn gray_image_size_bytes(image: &GrayImage) -> u64 {
    u64::from(image.width()).saturating_mul(u64::from(image.height()))
}

fn json_value_size_bytes(value: &Value) -> u64 {
    serde_json::to_vec(value).map(|bytes| bytes.len() as u64).unwrap_or(0)
}

fn daedalus_value_size_bytes(value: &DaedalusValue) -> u64 {
    daedalus_value_to_plain_json(value).map(|json| json_value_size_bytes(&json)).unwrap_or(0)
}

fn error_frame_like(image: &DynamicImage, title: &str, detail: Option<&str>) -> DynamicImage {
    let mut out = image.to_rgba8();
    let (width, height) = out.dimensions();
    if width == 0 || height == 0 {
        return DynamicImage::ImageRgba8(out);
    }

    let title = sanitize_error_text(title);
    let detail = detail.map(sanitize_error_text).filter(|s| !s.is_empty());
    let mut lines = vec![title];
    if let Some(detail) = detail {
        lines.push(detail);
    }

    draw_centered_text(&mut out, &lines);
    DynamicImage::ImageRgba8(out)
}

fn error_frame(dims: (u32, u32), title: &str, detail: Option<&str>) -> DynamicImage {
    let (width, height) = dims;
    let mut out = RgbaImage::from_pixel(width.max(1), height.max(1), Rgba([0, 0, 0, 255]));
    let mut lines = vec![sanitize_error_text(title)];
    if let Some(detail) = detail.map(sanitize_error_text).filter(|s| !s.is_empty()) {
        lines.push(detail);
    }
    draw_centered_text(&mut out, &lines);
    DynamicImage::ImageRgba8(out)
}

fn sanitize_error_text(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        let upper = ch.to_ascii_uppercase();
        if matches!(upper, 'A'..='Z' | '0'..='9' | ' ' | ':' | '-' | '_' | '.' | '/' | '(' | ')') {
            out.push(upper);
        } else if upper.is_whitespace() {
            out.push(' ');
        }
    }
    let trimmed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    trimmed.chars().take(64).collect()
}

fn format_panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(msg) = panic.downcast_ref::<&str>() {
        (*msg).to_string()
    } else if let Some(msg) = panic.downcast_ref::<String>() {
        msg.clone()
    } else {
        "unknown panic".to_string()
    }
}

fn format_error_detail(err_text: &str, node_type: Option<&str>) -> String {
    if let Some(node) = node_type {
        return format!("NODE: {node}");
    }
    err_text.to_string()
}

fn draw_centered_text(image: &mut RgbaImage, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    let (width, height) = image.dimensions();
    let min_dim = width.min(height) as f32;
    let mut scale = ((min_dim * 0.008).ceil() as u32).clamp(1, 6);
    let mut widths = Vec::new();

    let (line_gap, line_height, max_w, _total_h, pad, mut box_w, mut box_h) = loop {
        widths.clear();
        let mut max_w = 0u32;
        for line in lines {
            let w = text_width(line, scale);
            widths.push(w);
            max_w = max_w.max(w);
        }
        let line_gap = scale.saturating_div(2).max(1);
        let line_height = 7 * scale;
        let total_h = (lines.len() as u32 * line_height).saturating_add(line_gap.saturating_mul(lines.len().saturating_sub(1) as u32));
        let pad = scale.saturating_mul(2);
        let box_w = max_w.saturating_add(pad * 2);
        let box_h = total_h.saturating_add(pad * 2);
        if (box_w <= width && box_h <= height) || scale <= 1 {
            break (line_gap, line_height, max_w, total_h, pad, box_w, box_h);
        }
        scale = scale.saturating_sub(1).max(1);
    };
    if box_w > width {
        box_w = width;
    }
    if box_h > height {
        box_h = height;
    }
    let box_x = width.saturating_sub(box_w) / 2;
    let box_y = height.saturating_sub(box_h) / 2;

    fill_rect(image, box_x, box_y, box_w, box_h, Rgba([0, 0, 0, 255]));

    let mut y = box_y + pad;
    for (line, w) in lines.iter().zip(widths) {
        let x = box_x + pad + (max_w.saturating_sub(w) / 2);
        draw_text(image, line, x, y, scale, Rgba([255, 255, 255, 255]));
        y = y.saturating_add(line_height + line_gap);
    }
}

fn text_width(text: &str, scale: u32) -> u32 {
    let count = text.chars().count() as u32;
    if count == 0 {
        return 0;
    }
    count.saturating_mul((5 + 1) * scale).saturating_sub(scale)
}

fn fill_rect(image: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
    let max_x = (x + w).min(image.width());
    let max_y = (y + h).min(image.height());
    for yy in y..max_y {
        for xx in x..max_x {
            image.put_pixel(xx, yy, color);
        }
    }
}

fn draw_text(image: &mut RgbaImage, text: &str, x: u32, y: u32, scale: u32, color: Rgba<u8>) {
    let mut cursor_x = x;
    for ch in text.chars() {
        let rows = glyph_rows(ch);
        for (gy, row) in rows.iter().enumerate() {
            for gx in 0..5usize {
                if ((row >> (4 - gx)) & 1) == 0 {
                    continue;
                }
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = cursor_x + (gx as u32 * scale) + sx;
                        let py = y + (gy as u32 * scale) + sy;
                        if px < image.width() && py < image.height() {
                            image.put_pixel(px, py, color);
                        }
                    }
                }
            }
        }
        cursor_x = cursor_x.saturating_add((5 + 1) * scale);
    }
}

fn glyph_rows(c: char) -> [u8; 7] {
    match c {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '_' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => [0, 0, 0, 0, 0, 0, 0],
    }
}

fn infer_host_bridge_image_input_port(plan: &RuntimePlan, host_node_idx: usize, input_ports: &BTreeSet<String>) -> Option<String> {
    let node = plan.nodes.get(host_node_idx)?;
    let mut input_ports_by_lower: BTreeMap<String, String> = BTreeMap::new();
    for port in input_ports {
        input_ports_by_lower.entry(port.to_ascii_lowercase()).or_insert_with(|| port.clone());
    }

    let mut image_ports: BTreeSet<String> = BTreeSet::new();

    // Primary source: solved dynamic output types emitted by planner/runtime.
    if let Some(DaedalusValue::Map(items)) = node.metadata.get("dynamic_output_types") {
        for (key, value) in items {
            let (DaedalusValue::String(port_name), DaedalusValue::String(raw_ty)) = (key, value) else { continue };
            let Some(actual_port) = input_ports_by_lower.get(&port_name.to_ascii_lowercase()) else { continue };
            match serde_json::from_str::<DaedalusTypeExpr>(raw_ty) {
                Ok(ty) => {
                    if is_image_payload(&ty) {
                        image_ports.insert(actual_port.clone());
                    }
                }
                Err(_) => {
                    let raw_lower = raw_ty.to_ascii_lowercase();
                    if raw_lower == "image" || raw_lower.starts_with("image:") {
                        image_ports.insert(actual_port.clone());
                    }
                }
            }
        }
    }

    if image_ports.len() == 1 {
        return image_ports.iter().next().cloned();
    }

    // Deterministic fallback: `frame` is the canonical host camera image port.
    if let Some(frame_port) = input_ports.iter().find(|port| port.eq_ignore_ascii_case("frame")).cloned() {
        if image_ports.is_empty() || image_ports.iter().any(|port| port.eq_ignore_ascii_case("frame")) {
            return Some(frame_port);
        }
    }

    None
}

fn derive_host_aliases(plan: &RuntimePlan, host_mgr: &DaedalusBridgeManager) -> Result<(String, String, Vec<String>), GraphError> {
    let host_nodes: Vec<(usize, String)> = plan
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(idx, node)| {
            let is_bridge = matches!(node.metadata.get(HOST_BRIDGE_META_KEY), Some(DaedalusValue::Bool(true)));
            if !is_bridge {
                return None;
            }
            Some((idx, node.label.clone().unwrap_or_else(|| node.id.clone())))
        })
        .collect();
    if host_nodes.is_empty() {
        return Err(GraphError::MissingHostBridge);
    }

    let mut input_alias: Option<String> = None;
    let mut input_node_idx: Option<usize> = None;
    let mut input_ports: BTreeSet<String> = BTreeSet::new();
    let mut output_aliases: Vec<String> = Vec::new();

    // Optional explicit selection: if the graph authors want a specific host output port
    // to be the "stream input", they can set this metadata on the host bridge node.
    const HELIOS_HOST_INPUT_PORT_KEY: &str = "helios.host_input_port";

    let mut explicit_input_port: Option<String> = None;
    for (idx, alias) in &host_nodes {
        let outgoing_ports: BTreeSet<String> = plan.edges.iter().filter(|(from, _, _, _, _)| from.0 == *idx).map(|(_, from_port, _, _, _)| from_port.clone()).collect();
        let has_outgoing = !outgoing_ports.is_empty();
        let has_incoming = plan.edges.iter().any(|(_, _, to, _, _)| to.0 == *idx);
        tracing::debug!(
            host_alias = %alias,
            host_index = *idx,
            has_outgoing,
            has_incoming,
            outgoing_ports = ?outgoing_ports,
            "daedalus graph: host bridge ports"
        );
        if explicit_input_port.is_none() {
            if let Some(DaedalusValue::String(port)) = plan.nodes.get(*idx).and_then(|n| n.metadata.get(HELIOS_HOST_INPUT_PORT_KEY)) {
                let trimmed = port.as_ref().trim();
                if !trimmed.is_empty() {
                    explicit_input_port = Some(trimmed.to_string());
                }
            }
        }
        if has_outgoing && input_alias.is_none() {
            input_alias = Some(alias.clone());
            input_node_idx = Some(*idx);
            input_ports = outgoing_ports;
        }
        if has_incoming {
            output_aliases.push(alias.clone());
        }
    }

    let input_alias = input_alias.ok_or(GraphError::MissingHostBridge)?;
    if host_mgr.handle(&input_alias).is_none() {
        return Err(GraphError::MissingHostBridge);
    }
    let input_port = if let Some(explicit) = explicit_input_port {
        input_ports.iter().find(|p| p.eq_ignore_ascii_case(&explicit)).cloned().ok_or_else(|| {
            let ports: Vec<_> = input_ports.iter().cloned().collect();
            GraphError::Build(format!("graph host bridge input port {:?} not found (ports={ports:?}); set {} to a valid outgoing port", explicit, HELIOS_HOST_INPUT_PORT_KEY))
        })?
    } else if input_ports.len() == 1 {
        input_ports.iter().next().cloned().ok_or_else(|| GraphError::Build("graph host bridge has no outgoing ports".to_string()))?
    } else if let Some(node_idx) = input_node_idx {
        if let Some(inferred) = infer_host_bridge_image_input_port(plan, node_idx, &input_ports) {
            tracing::debug!(input_host = %input_alias, input_port = %inferred, "daedalus graph: resolved host input by inferred port type");
            inferred
        } else {
            let ports: Vec<_> = input_ports.iter().cloned().collect();
            return Err(GraphError::Build(format!("graph host bridge input port is ambiguous (ports={ports:?}); set {} metadata on the host bridge node", HELIOS_HOST_INPUT_PORT_KEY)));
        }
    } else {
        let ports: Vec<_> = input_ports.iter().cloned().collect();
        return Err(GraphError::Build(format!("graph host bridge input port is ambiguous (ports={ports:?}); set {} metadata on the host bridge node", HELIOS_HOST_INPUT_PORT_KEY)));
    };
    tracing::debug!(input_host = %input_alias, input_port = %input_port, "daedalus graph: resolved host input");

    if output_aliases.is_empty() {
        return Err(GraphError::MissingHostBridge);
    }

    Ok((input_alias, input_port, output_aliases))
}

fn host_output_sink_node_index(plan: &RuntimePlan, output_hosts: &[String]) -> Option<usize> {
    let mut incoming = vec![false; plan.nodes.len()];
    for (_, _, to, _, _) in &plan.edges {
        if to.0 < incoming.len() {
            incoming[to.0] = true;
        }
    }

    for alias in output_hosts {
        if let Some((idx, _)) = plan.nodes.iter().enumerate().find(|(idx, node)| {
            incoming.get(*idx).copied().unwrap_or(false)
                && (node.id == "io.host_output" || node.id.ends_with(":io.host_output"))
                && node.label.as_deref().is_some_and(|label| label.eq_ignore_ascii_case(alias))
        }) {
            return Some(idx);
        }
    }

    plan.nodes.iter().enumerate().find_map(|(idx, node)| {
        if !incoming.get(idx).copied().unwrap_or(false) {
            return None;
        }
        if node.id == "io.host_output" || node.id.ends_with(":io.host_output") {
            Some(idx)
        } else {
            None
        }
    })
}

fn infer_host_output_port_owners(plan: &RuntimePlan, output_hosts: &[String]) -> BTreeMap<String, usize> {
    let mut owners = BTreeMap::new();
    for (from, _from_port, to, to_port, _) in &plan.edges {
        let _ = from;
        let Some(to_node) = plan.nodes.get(to.0) else {
            continue;
        };
        if !(to_node.id == "io.host_output" || to_node.id.ends_with(":io.host_output")) {
            continue;
        }
        if !output_hosts.is_empty() && !output_hosts.iter().any(|alias| to_node.label.as_deref().is_some_and(|label| label.eq_ignore_ascii_case(alias))) {
            continue;
        }
        let key = to_port.to_ascii_lowercase();
        // Demand-driven masking must target the host-output sink and preserve the selected input
        // port. Daedalus will then walk only that port's upstream closure. Pointing this at the
        // producer node skips `io.host_output` entirely, which makes preview publication disappear.
        owners.entry(key).or_insert(to.0);
    }
    owners
}

fn build_demand_mask(
    plan: &RuntimePlan,
    output_hosts: &[String],
    preview_ports: &[String],
    host_output_ports: &[String],
    host_output_port_types: &BTreeMap<String, DaedalusTypeExpr>,
    host_output_port_owners: &BTreeMap<String, usize>,
    demand_driven: bool,
    include_preview_ports: bool,
    include_value_ports: bool,
    extra_sink_ports: &[String],
) -> Option<Vec<bool>> {
    if !demand_driven {
        return None;
    }

    let fallback_selector = if let Some(index) = host_output_sink_node_index(plan, output_hosts) {
        daedalus::planner::GraphNodeSelector { index: Some(index), id: None, metadata: None }
    } else {
        daedalus::planner::GraphNodeSelector { index: None, id: Some("io.host_output".to_string()), metadata: None }
    };

    let mut sink_ports: BTreeSet<String> = BTreeSet::new();
    if include_preview_ports {
        for port in preview_ports {
            if !port.trim().is_empty() {
                sink_ports.insert(port.clone());
            }
        }
    }
    if include_value_ports {
        // Demand-driven execution still needs non-preview host outputs (for example `detections`)
        // so sampling and calibration solve can read value ports from the same graph tick.
        for port in host_output_ports {
            if port.trim().is_empty() {
                continue;
            }
            let key = port.to_ascii_lowercase();
            if sink_ports.contains(port) {
                continue;
            }
            let is_image = host_output_port_types.get(&key).map(is_image_payload).unwrap_or(false);
            if !is_image {
                sink_ports.insert(port.clone());
            }
        }
    }
    for port in extra_sink_ports {
        if !port.trim().is_empty() {
            sink_ports.insert(port.clone());
        }
    }

    let mut sinks = Vec::new();
    for port in sink_ports {
        let key = port.to_ascii_lowercase();
        let selector =
            host_output_port_owners.get(&key).copied().map(|index| daedalus::planner::GraphNodeSelector { index: Some(index), id: None, metadata: None }).unwrap_or_else(|| fallback_selector.clone());
        sinks.push(RuntimeSink { node: selector, port: Some(port) });
    }
    if sinks.is_empty() {
        None
    } else {
        plan.active_nodes_for_sinks(&sinks).ok()
    }
}

#[cfg(test)]
fn build_demand_sinks(
    plan: &RuntimePlan,
    output_hosts: &[String],
    preview_ports: &[String],
    host_output_ports: &[String],
    host_output_port_types: &BTreeMap<String, DaedalusTypeExpr>,
    host_output_port_owners: &BTreeMap<String, usize>,
    demand_driven: bool,
    extra_sink_ports: &[String],
) -> Vec<RuntimeSink> {
    if !demand_driven {
        return Vec::new();
    }

    let fallback_selector = if let Some(index) = host_output_sink_node_index(plan, output_hosts) {
        daedalus::planner::GraphNodeSelector { index: Some(index), id: None, metadata: None }
    } else {
        daedalus::planner::GraphNodeSelector { index: None, id: Some("io.host_output".to_string()), metadata: None }
    };

    let mut sink_ports: BTreeSet<String> = BTreeSet::new();
    for port in preview_ports {
        if !port.trim().is_empty() {
            sink_ports.insert(port.clone());
        }
    }
    for port in host_output_ports {
        if port.trim().is_empty() {
            continue;
        }
        let key = port.to_ascii_lowercase();
        if sink_ports.contains(port) {
            continue;
        }
        let is_image = host_output_port_types.get(&key).map(is_image_payload).unwrap_or(false);
        if !is_image {
            sink_ports.insert(port.clone());
        }
    }
    for port in extra_sink_ports {
        if !port.trim().is_empty() {
            sink_ports.insert(port.clone());
        }
    }

    sink_ports
        .into_iter()
        .map(|port| {
            let key = port.to_ascii_lowercase();
            let selector = host_output_port_owners
                .get(&key)
                .copied()
                .map(|index| daedalus::planner::GraphNodeSelector { index: Some(index), id: None, metadata: None })
                .unwrap_or_else(|| fallback_selector.clone());
            RuntimeSink { node: selector, port: Some(port) }
        })
        .collect()
}

fn plan_uses_gpu(plan: &RuntimePlan) -> bool {
    plan.segments.iter().any(|segment| !matches!(segment.compute, ComputeAffinity::CpuOnly))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn extract_node_type(error: &str) -> Option<String> {
    let needle = "node: \"";
    let start = error.find(needle)? + needle.len();
    let rest = &error[start..];
    let end = rest.find('"')?;
    let node = rest[..end].trim();
    if node.is_empty() {
        None
    } else {
        Some(node.to_string())
    }
}
