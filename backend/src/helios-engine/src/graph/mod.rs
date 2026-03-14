use daedalus::data::model::StructFieldValue;
use daedalus::data::model::TypeExpr as DaedalusTypeExpr;
use daedalus::data::model::Value as DaedalusValue;
use daedalus::engine::{Engine, EngineConfig, RuntimeMode};
use daedalus::gpu::{select_backend, ErasedPayload, GpuBackendKind, GpuContextHandle, GpuOptions};
use daedalus::planner::{ComputeAffinity, Graph, GraphPatch, PatchReport};
use daedalus::runtime::executor::EdgePayload as DaedalusEdgePayload;
use daedalus::runtime::executor::ExecutionTelemetry as DaedalusExecutionTelemetry;
use daedalus::runtime::executor::OwnedExecutor as DaedalusOwnedExecutor;
use daedalus::runtime::handler_registry::HandlerRegistry as DaedalusHandlers;
use daedalus::runtime::host_bridge::HOST_BRIDGE_META_KEY;
use daedalus::runtime::{BackpressureStrategy, EdgePolicyKind, HostBridgeManager as DaedalusBridgeManager, RuntimePlan, RuntimeSink};
use daedalus::Payload;
use image::{DynamicImage, GrayImage, Rgba, RgbaImage};
use lib_cv::modules::aruco::ArucoDetection2D;
use metrics::histogram;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
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
use crate::stream::{PipelineFlamegraphMetrics, PipelineGraphMetrics, PipelineNodeMetrics, PipelineNodePerfMetrics, PipelineNodeRuntimeMetrics, PipelinePerfMetrics, PipelineSampleCacheMetrics};

mod builder;
pub(crate) mod context;
mod daedalus_config;
mod flamegraph;
mod multiplex;
mod perf;
#[cfg(test)]
mod tests;

use self::daedalus_config::{apply_daedalus_engine_config_overrides, apply_daedalus_engine_env_overrides, KEY_RUNTIME_BACKPRESSURE, KEY_RUNTIME_DEFAULT_POLICY};
pub(crate) use builder::{build_graph_handle_for_manifest, build_graph_handle_for_pipeline_output};

fn is_image_payload(ty: &DaedalusTypeExpr) -> bool {
    match ty {
        DaedalusTypeExpr::Opaque(name) => {
            let lower = name.to_ascii_lowercase();
            lower == "image" || lower.starts_with("image:")
        }
        DaedalusTypeExpr::Optional(inner) => is_image_payload(inner.as_ref()),
        _ => false,
    }
}

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

pub trait GraphExecutor: Send + Sync {
    /// Process an incoming frame and optionally emit a transformed frame.
    fn process(&self, image: DynamicImage) -> Option<DynamicImage>;

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
}

impl std::fmt::Debug for GraphHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphHandle").field("executor_present", &self.executor.is_some()).finish()
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
        // Ordering defaults to no-op.
        "order_mode" => Some(DaedalusValue::String("none".into())),
        _ => None,
    }
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

fn normalize_graph_json_for_runtime(json: &Value) -> Value {
    let mut normalized = json.clone();
    normalize_graph_metadata(&mut normalized);

    let Some(obj) = normalized.as_object_mut() else {
        return normalized;
    };
    let Some(nodes) = obj.get_mut("nodes").and_then(|v| v.as_array_mut()) else {
        return normalized;
    };
    for node in nodes {
        let Some(node_obj) = node.as_object_mut() else { continue };
        ensure_host_bridge_node_shape(node_obj);
    }
    normalized
}

fn normalize_graph_enum_const_inputs(graph: &mut Graph, registry: &daedalus::runtime::plugins::PluginRegistry) {
    fn unwrap_legacy_typed_value(value: &DaedalusValue) -> Option<DaedalusValue> {
        let mut ty: Option<String> = None;
        let mut raw: Option<DaedalusValue> = None;
        match value {
            // Older graphs occasionally stored consts as generic map payloads.
            DaedalusValue::Map(items) => {
                for (k, v) in items {
                    let DaedalusValue::String(name) = k else {
                        continue;
                    };
                    match name.as_ref() {
                        "type" => {
                            if let DaedalusValue::String(kind) = v {
                                ty = Some(kind.to_string());
                            }
                        }
                        "value" => raw = Some(v.clone()),
                        _ => {}
                    }
                }
            }
            // Common JSON form (`{\"type\": ..., \"value\": ...}`) can deserialize to Struct.
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

    // Convert enum constants to index-based ints (Daedalus now accepts enums by discriminant).
    // Legacy graphs may still provide strings; map them to the registered variant order.
    let view = registry.registry.view();
    for node in &mut graph.nodes {
        let Some(desc) = view.nodes.get(&daedalus::registry::ids::NodeId(node.id.0.clone())) else {
            continue;
        };
        for (port, value) in &mut node.const_inputs {
            if let Some(unwrapped) = unwrap_legacy_typed_value(value) {
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
        let Some(mut suffix) = name_lc.strip_prefix(&prefix_lc) else { continue };
        if let Some(stripped) = suffix.strip_suffix('+') {
            suffix = stripped;
        }
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

    // Canonicalize fan-in edge target ports (e.g. `sources0+` -> `sources0`) so planner
    // validation/typecheck uses the same naming convention as registry `input_ty_for`.
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

        // Canonicalize fan-in input declarations/consts (legacy `sources0+` => `sources0`).
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
        Self { host, executor: None }
    }

    pub fn with_executor(host: HostBridgeHandle, executor: Arc<dyn GraphExecutor>) -> Self {
        Self { host, executor: Some(executor) }
    }

    pub fn with_default_host(buffer: usize) -> Self {
        let (host, rx) = HostBridgeHandle::new(buffer);
        let _ = rx;
        Self { host, executor: None }
    }

    /// Build a graph handle from a Daedalus graph JSON payload. This validates the graph
    /// and installs a Daedalus-backed executor so host frames are routed through the graph.
    pub fn from_json(buffer: usize, json: &Value) -> Result<Self, GraphError> {
        Self::from_json_with_pool(buffer, json, pool_size_from_env(), None)
    }

    /// Same as `from_json` but optionally selects a single host output port to forward.
    pub fn from_json_with_output(buffer: usize, json: &Value, output_port: Option<&str>) -> Result<Self, GraphError> {
        Self::from_json_with_pool(buffer, json, pool_size_from_env(), output_port)
    }

    /// Same as `from_json` but allows overriding the Daedalus executor pool size.
    pub fn from_json_with_pool(buffer: usize, json: &Value, pool_size: Option<usize>, output_port: Option<&str>) -> Result<Self, GraphError> {
        let normalized = normalize_graph_json_for_runtime(json);
        let graph: Graph = serde_json::from_value(normalized).map_err(|e| GraphError::Parse(e.to_string()))?;
        let (host, rx) = HostBridgeHandle::new(buffer);
        let _ = rx;
        let executor = DaedalusGraphExecutor::new(graph, pool_size, output_port.map(str::to_string))?;
        Ok(Self { host, executor: Some(Arc::new(executor)) })
    }

    pub fn process(&self, image: DynamicImage) -> Option<DynamicImage> {
        if let Some(exec) = &self.executor {
            exec.process(image)
        } else {
            Some(image)
        }
    }

    pub fn set_calibration(&self, calibration: Option<crate::ipc::StreamCalibration>) {
        if let Some(exec) = &self.executor {
            let should_clear = calibration.is_some();
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
        exec.apply_graph_patch(pipeline_id, patch)
    }

    pub fn set_pipeline_input_values(&self, pipeline_id: Option<uuid::Uuid>, inputs: &BTreeMap<String, Option<DaedalusValue>>) {
        if let Some(exec) = &self.executor {
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

    pub fn pipeline_metrics(&self) -> Option<PipelineGraphMetrics> {
        self.executor.as_ref().and_then(|exec| exec.pipeline_metrics())
    }

    pub fn pipeline_metrics_by_pipeline(&self) -> Option<BTreeMap<String, PipelineGraphMetrics>> {
        self.executor.as_ref().and_then(|exec| exec.pipeline_metrics_by_pipeline())
    }

    pub fn host_output_ports(&self) -> Option<Vec<String>> {
        self.executor.as_ref().and_then(|exec| exec.host_output_ports())
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

    pub fn sample_json_output(&self, port: &str) -> Option<Value> {
        self.executor.as_ref().and_then(|exec| exec.sample_json_output(port))
    }

    pub fn sample_value_output(&self, port: &str) -> Option<DaedalusValue> {
        self.executor.as_ref().and_then(|exec| exec.sample_value_output(port))
    }

    pub fn sample_image_output(&self, port: &str) -> Option<DynamicImage> {
        self.executor.as_ref().and_then(|exec| exec.sample_image_output(port))
    }

    pub fn disabled_state(&self) -> GraphDisabledState {
        self.executor.as_ref().map(|exec| exec.disabled_state()).unwrap_or_default()
    }

    pub fn clear_disabled(&self) {
        if let Some(exec) = &self.executor {
            exec.clear_disabled();
        }
    }

    pub fn set_perf_enabled(&self, pipeline_id: Option<uuid::Uuid>, enabled: bool) {
        if let Some(exec) = &self.executor {
            exec.set_perf_enabled(pipeline_id, enabled);
        }
    }

    pub fn reset_pipeline_metrics(&self, pipeline_id: Option<uuid::Uuid>) {
        if let Some(exec) = &self.executor {
            exec.reset_pipeline_metrics(pipeline_id);
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
    output_hosts: Vec<String>,
    host_output_ports: Vec<String>,
    host_output_ports_lc: BTreeSet<String>,
    /// Solved types for declared host output ports (keyed by lowercase port name).
    host_output_port_types: BTreeMap<String, DaedalusTypeExpr>,
    host_output_port_owners: BTreeMap<String, usize>,
    preview_ports: Vec<String>,
    preview_ports_lc: BTreeSet<String>,
    run_mode: RuntimeMode,
    executor: Arc<std::sync::Mutex<DaedalusOwnedExecutor<DaedalusHandlers>>>,
    metrics: Mutex<RollingGraphMetrics>,
    json_samples: Mutex<BTreeMap<String, Value>>,
    value_samples: Mutex<BTreeMap<String, DaedalusValue>>,
    image_samples: Mutex<BTreeMap<String, DynamicImage>>,
    process_calls: AtomicU64,
    perf_enabled: AtomicBool,
    pprof_pending: AtomicBool,
    pprof_remaining: AtomicU64,
    /// Wall-clock deadline for flamegraph capture (ms since epoch). 0 disables.
    pprof_until_ms: AtomicU64,
    pprof_guard: Mutex<Option<flamegraph::FlamegraphGuard>>,
    calibration_payload: std::sync::RwLock<DaedalusValue>,
    input_values: std::sync::RwLock<BTreeMap<String, DaedalusValue>>,
    last_error_detail: std::sync::RwLock<String>,
    failure_count: AtomicU64,
    disabled: AtomicBool,
    disabled_since_ms: AtomicU64,
    rebuild_requested: AtomicBool,
}

const NODE_METRICS_WINDOW: usize = 100;
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

#[derive(Debug, Default)]
struct RollingGraphMetrics {
    window: usize,
    node_info: Vec<NodeInfo>,
    edge_info: Vec<EdgeInfo>,
    samples: BTreeMap<usize, VecDeque<(Instant, f64)>>,
    node_perf_samples: BTreeMap<usize, VecDeque<(Instant, NodePerfSample)>>,
    edge_samples: BTreeMap<usize, VecDeque<(Instant, daedalus::runtime::executor::EdgeMetrics)>>,
    group_samples: BTreeMap<String, VecDeque<(Instant, f64)>>,
    group_perf_samples: BTreeMap<String, VecDeque<(Instant, NodePerfSample)>>,
    graph_samples: VecDeque<(Instant, f64)>,
    perf_samples: VecDeque<(Instant, perf::PerfSample)>,
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
            edge_samples: BTreeMap::new(),
            group_samples: BTreeMap::new(),
            group_perf_samples: BTreeMap::new(),
            graph_samples: VecDeque::new(),
            perf_samples: VecDeque::new(),
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
                perf_deque.push_back((now, sample));
                while perf_deque.len() > self.window {
                    perf_deque.pop_front();
                }
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
                perf_deque.push_back((now, sample));
                while perf_deque.len() > self.window {
                    perf_deque.pop_front();
                }
            }
        }
        for (edge_idx, edge_metrics) in &telemetry.edge_metrics {
            let deque = self.edge_samples.entry(*edge_idx).or_default();
            deque.push_back((now, edge_metrics.clone()));
            while deque.len() > self.window {
                deque.pop_front();
            }
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

    fn record_perf_sample(&mut self, sample: perf::PerfSample) {
        let now = Instant::now();
        self.perf_samples.push_back((now, sample));
        while self.perf_samples.len() > self.window {
            self.perf_samples.pop_front();
        }
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
        self.edge_samples.clear();
        self.group_samples.clear();
        self.group_perf_samples.clear();
        self.graph_samples.clear();
        self.perf_samples.clear();
        self.last_flamegraph = None;
        self.warnings.clear();
        self.last_errors.clear();
    }

    fn snapshot(&self) -> PipelineGraphMetrics {
        let now = Instant::now();
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
                let sum_cache: f64 = perf_deque.iter().map(|(_, sample)| sample.cache_misses).sum();
                let sum_branch_inst: f64 = perf_deque.iter().map(|(_, sample)| sample.branch_instructions).sum();
                let sum_branch_miss: f64 = perf_deque.iter().map(|(_, sample)| sample.branch_misses).sum();
                let last_age_ms = perf_deque.back().map(|(t, _)| now.saturating_duration_since(*t).as_millis() as u64);
                Some(PipelineNodePerfMetrics {
                    average_cache_misses: sum_cache / perf_sample_count.max(1) as f64,
                    average_branch_instructions: sum_branch_inst / perf_sample_count.max(1) as f64,
                    average_branch_misses: sum_branch_miss / perf_sample_count.max(1) as f64,
                    sample_count: perf_sample_count,
                    window_size: self.window as u64,
                    last_sample_age_ms: last_age_ms,
                })
            });
            out.insert(
                key,
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms, average_fps, sample_count, window_size: self.window as u64, last_sample_age_ms },
                    perf,
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
                let sum_cache: f64 = perf_deque.iter().map(|(_, sample)| sample.cache_misses).sum();
                let sum_branch_inst: f64 = perf_deque.iter().map(|(_, sample)| sample.branch_instructions).sum();
                let sum_branch_miss: f64 = perf_deque.iter().map(|(_, sample)| sample.branch_misses).sum();
                let last_age_ms = perf_deque.back().map(|(t, _)| now.saturating_duration_since(*t).as_millis() as u64);
                Some(PipelineNodePerfMetrics {
                    average_cache_misses: sum_cache / perf_sample_count.max(1) as f64,
                    average_branch_instructions: sum_branch_inst / perf_sample_count.max(1) as f64,
                    average_branch_misses: sum_branch_miss / perf_sample_count.max(1) as f64,
                    sample_count: perf_sample_count,
                    window_size: self.window as u64,
                    last_sample_age_ms: last_age_ms,
                })
            });
            group_entries.insert(
                group_id.clone(),
                PipelineNodeRuntimeMetrics {
                    metrics: PipelineNodeMetrics { average_time_ms, average_fps, sample_count, window_size: self.window as u64, last_sample_age_ms },
                    perf,
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

            let wait_sample_count: u64 = deque.iter().map(|(_, metrics)| metrics.samples as u64).sum();
            let total_wait_ms: f64 = deque.iter().map(|(_, metrics)| metrics.total_wait.as_secs_f64() * 1000.0).sum();
            let payload_count: u64 = deque.iter().map(|(_, metrics)| metrics.payload_count).sum();
            let payload_bytes: u64 = deque.iter().map(|(_, metrics)| metrics.payload_bytes).sum();
            let max_depth = deque.iter().map(|(_, metrics)| metrics.max_depth).max().unwrap_or(0);
            let dropped = deque.iter().map(|(_, metrics)| metrics.drops).sum();
            let gpu_uploads = deque.iter().map(|(_, metrics)| metrics.gpu_uploads).sum();
            let gpu_downloads = deque.iter().map(|(_, metrics)| metrics.gpu_downloads).sum();
            let current_depth = deque.back().map(|(_, metrics)| metrics.current_depth).unwrap_or(0);
            let average_wait_ms = if wait_sample_count > 0 { total_wait_ms / wait_sample_count as f64 } else { 0.0 };
            let average_payload_bytes = if payload_count > 0 { payload_bytes as f64 / payload_count as f64 } else { 0.0 };
            let last_sample_age_ms = deque.back().map(|(t, _)| now.saturating_duration_since(*t).as_millis() as u64);
            let info = self.edge_info.get(*edge_idx);
            let capacity = deque.iter().filter_map(|(_, metrics)| metrics.capacity).max().or_else(|| info.and_then(|edge| edge.queue_capacity));

            edge_entries.insert(
                format!("edge_{edge_idx}"),
                crate::stream::PipelineEdgeRuntimeMetrics {
                    average_wait_ms,
                    wait_sample_count,
                    window_size: self.window as u64,
                    last_sample_age_ms,
                    max_depth,
                    current_depth,
                    current_queue_bytes: deque.back().map(|(_, metrics)| metrics.current_queue_bytes).unwrap_or(0),
                    peak_queue_bytes: deque.iter().map(|(_, metrics)| metrics.peak_queue_bytes).max().unwrap_or(0),
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
            let sum_cache: f64 = self.perf_samples.iter().map(|(_, sample)| sample.cache_misses as f64).sum();
            let sum_branch_inst: f64 = self.perf_samples.iter().map(|(_, sample)| sample.branch_instructions as f64).sum();
            let sum_branch_miss: f64 = self.perf_samples.iter().map(|(_, sample)| sample.branch_misses as f64).sum();
            let last_age_ms = self.perf_samples.back().map(|(t, _)| now.saturating_duration_since(*t).as_millis() as u64);
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

        PipelineGraphMetrics {
            nodes: out,
            groups: if root_groups.is_empty() { None } else { Some(root_groups) },
            edges: if edge_entries.is_empty() { None } else { Some(edge_entries) },
            sample_cache: None,
            perf,
            flamegraph,
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
        normalize_graph_enum_const_inputs(&mut graph, &registry);
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
                let queue_cap = env::var("HELIOS_DAEDALUS_RUNTIME_QUEUE_CAP").ok().and_then(|raw| raw.parse::<usize>().ok()).filter(|cap| *cap > 0).unwrap_or(4);
                cfg.runtime.default_policy = EdgePolicyKind::Bounded { cap: queue_cap };
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

        let host_outputs_in_graph = host_outputs_in_graph_enabled(Some(plan.as_ref()));
        let demand_driven = demand_driven_enabled(Some(plan.as_ref()));
        let mut executor = DaedalusOwnedExecutor::new(plan.clone(), handlers.clone_arc())
            .with_host_bridges(host_mgr.clone())
            .with_const_coercers(const_coercers.clone())
            .with_output_movers(output_movers.clone())
            // Daedalus error-isolation: keep the graph running and surface errors via telemetry
            // instead of killing the whole run on the first failing node.
            .with_fail_fast(false)
            // Host output execution can be moved "in graph" for responsiveness, but this changes
            // scheduling semantics and can cause missing outputs depending on executor ordering.
            // Keep it opt-in until Daedalus scheduling guarantees sink ordering.
            .with_host_outputs_in_graph(host_outputs_in_graph);

        let preview_ports_lc: BTreeSet<String> = preview_ports.iter().map(|p| p.to_ascii_lowercase()).collect();
        let demand_sinks = build_demand_sinks(plan.as_ref(), &output_hosts, &preview_ports, &host_output_ports, &host_output_port_types, demand_driven);
        if !demand_sinks.is_empty() {
            executor = executor.with_demand_sinks(demand_sinks);
        }
        if let Some(handle) = gpu.clone() {
            executor = executor.with_gpu(handle);
        }
        if let Some(size) = pool_size {
            executor = executor.with_pool_size(Some(size));
        }
        let dedicated_executor = dedicated_executor_from_env();
        let busy_behavior = executor_busy_behavior_from_env();
        let busy_timeout = executor_busy_timeout_from_env();

        tracing::info!(
            input_host = %input_host_alias,
            input_port = %input_port,
            output_hosts = ?output_hosts,
            host_output_ports = ?host_output_ports,
            preview_ports = ?preview_ports,
            "daedalus graph: host ports configured"
        );

        let pprof_enabled = pprof_enabled_from_env();
        let pprof_duration_ms = if pprof_enabled { pprof_duration_ms_from_env() } else { None };
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
            output_hosts,
            host_output_ports,
            host_output_ports_lc,
            host_output_port_types,
            host_output_port_owners,
            preview_ports,
            preview_ports_lc,
            run_mode: engine.config().runtime.mode.clone(),
            executor: Arc::new(std::sync::Mutex::new(executor)),
            metrics: Mutex::new(RollingGraphMetrics::new(NODE_METRICS_WINDOW, node_info, edge_info)),
            json_samples: Mutex::new(BTreeMap::new()),
            value_samples: Mutex::new(BTreeMap::new()),
            image_samples: Mutex::new(BTreeMap::new()),
            process_calls: AtomicU64::new(0),
            perf_enabled: AtomicBool::new(perf_counters_enabled_from_env()),
            pprof_pending: AtomicBool::new(pprof_enabled),
            pprof_remaining: AtomicU64::new(if pprof_enabled && pprof_until_ms == 0 { pprof_frames_from_env() } else { 0 }),
            pprof_until_ms: AtomicU64::new(pprof_until_ms),
            pprof_guard: Mutex::new(None),
            calibration_payload: std::sync::RwLock::new(calibration_to_daedalus_value(None)),
            input_values: std::sync::RwLock::new(BTreeMap::new()),
            last_error_detail: std::sync::RwLock::new(String::new()),
            failure_count: AtomicU64::new(0),
            disabled: AtomicBool::new(false),
            disabled_since_ms: AtomicU64::new(0),
            rebuild_requested: AtomicBool::new(false),
        })
    }

    fn rebuild_shared_executor(&self) -> Result<(), String> {
        let host_outputs_in_graph = host_outputs_in_graph_enabled(Some(self.plan.as_ref()));
        let demand_driven = demand_driven_enabled(Some(self.plan.as_ref()));
        let mut executor = DaedalusOwnedExecutor::new(self.plan.clone(), self.handlers.clone_arc())
            .with_host_bridges(self.host_mgr.clone())
            .with_const_coercers(self.const_coercers.clone())
            .with_output_movers(self.output_movers.clone())
            .with_fail_fast(false)
            .with_host_outputs_in_graph(host_outputs_in_graph);
        let demand_sinks = build_demand_sinks(self.plan.as_ref(), &self.output_hosts, &self.preview_ports, &self.host_output_ports, &self.host_output_port_types, demand_driven);
        if !demand_sinks.is_empty() {
            executor = executor.with_demand_sinks(demand_sinks);
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
        let call_idx = self.process_calls.fetch_add(1, Ordering::Relaxed);
        if call_idx < 3 {
            tracing::debug!(call_idx, "daedalus graph: processing frame");
        }
        if self.disabled.load(Ordering::Relaxed) {
            let disabled_since = self.disabled_since_ms.load(Ordering::Relaxed);
            if call_idx < 3 || call_idx.is_multiple_of(120) {
                tracing::warn!(call_idx, disabled_since, "graph disabled after repeated errors; emitting error frame");
            }
            let detail = self.last_error_detail.read().ok().map(|guard| guard.trim().to_string()).filter(|text| !text.is_empty());
            return Some(error_frame_like(&image, "GRAPH DISABLED", detail.as_deref()));
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
        // Keep a copy of the input image so we can fall back to passthrough when the graph fails.
        let input_image = image.clone();
        for alias in &self.output_hosts {
            let Some(output_host) = self.host_mgr.handle(alias) else { continue };
            for port in output_host.incoming_port_names() {
                let _ = output_host.drain(&port);
            }
        }
        let input_host = self.host_mgr.handle(&self.input_host_alias)?;
        let pushed = DaedalusEdgePayload::Payload(ErasedPayload::from_cpu::<DynamicImage>(image));
        let correlation_id = input_host.push(&self.input_port, pushed, None);
        if call_idx < 3 {
            tracing::debug!(call_idx, correlation_id, port = %self.input_port, "daedalus graph: pushed input");
        }
        let calibration_port = self.calibration_port.clone().or_else(|| input_host.outgoing_ports().find(|p| p.eq_ignore_ascii_case("calibration")).map(|p| p.to_string()));
        let mut provided_inputs: BTreeSet<String> = BTreeSet::new();
        provided_inputs.insert(self.input_port.to_ascii_lowercase());

        if let Some(port) = calibration_port.as_deref() {
            let payload = self.calibration_payload.read().ok().map(|guard| guard.clone()).unwrap_or_else(|| calibration_to_daedalus_value(None));
            let pushed = DaedalusEdgePayload::Value(payload);
            let _ = input_host.push(port, pushed, Some(correlation_id));
            provided_inputs.insert(port.to_ascii_lowercase());
        }
        if let Ok(guard) = self.input_values.read() {
            for (port, value) in guard.iter() {
                if port.eq_ignore_ascii_case(&self.input_port) {
                    continue;
                }
                if let Some(cal_port) = calibration_port.as_deref() {
                    if port.eq_ignore_ascii_case(cal_port) {
                        continue;
                    }
                }
                let pushed = DaedalusEdgePayload::Value(value.clone());
                let _ = input_host.push(port, pushed, Some(correlation_id));
                provided_inputs.insert(port.to_ascii_lowercase());
            }
        }
        for port in input_host.outgoing_ports() {
            let key = port.to_ascii_lowercase();
            if provided_inputs.contains(&key) {
                continue;
            }
            let Some(default_value) = default_host_bridge_input_value(&key) else {
                continue;
            };
            let pushed = DaedalusEdgePayload::Value(default_value);
            let _ = input_host.push(port, pushed, Some(correlation_id));
            if call_idx < 3 {
                tracing::debug!(call_idx, port = %port, "daedalus graph: pushed default host input");
            }
        }

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

        let run_result: Result<(DaedalusExecutionTelemetry, Duration), String> = if self.dedicated_executor {
            let host_outputs_in_graph = host_outputs_in_graph_enabled(Some(self.plan.as_ref()));
            let mut exec = DaedalusOwnedExecutor::new(self.plan.clone(), self.handlers.clone_arc())
                .with_host_bridges(self.host_mgr.clone())
                .with_const_coercers(self.const_coercers.clone())
                .with_output_movers(self.output_movers.clone())
                .with_fail_fast(false)
                .with_host_outputs_in_graph(host_outputs_in_graph);
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

            let lock_ms = lock_start.elapsed().as_secs_f64() * 1000.0;
            histogram!("helios.stream.executor_lock_ms").record(lock_ms);

            let exec = exec.as_deref_mut()?;

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
                return Some(error_frame_like(&input_image, "GRAPH ERROR", Some(&detail)));
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
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.record_graph_duration(run_duration);
            metrics.record_telemetry(&telemetry);
            if let Some(sample) = perf_sample {
                metrics.record_perf_sample(sample);
            }
            if let Some(capture) = flamegraph_capture {
                metrics.record_flamegraph(capture);
            }
        }

        let mut preview_image: Option<DynamicImage> = None;
        let mut preview_key: Option<String> = None;
        let mut image_updates: Vec<(String, DynamicImage)> = Vec::new();
        let mut json_updates: Vec<(String, Value)> = Vec::new();
        let mut value_updates: Vec<(String, DaedalusValue)> = Vec::new();
        let mut popped_outputs = 0usize;

        for alias in &self.output_hosts {
            let Some(output_host) = self.host_mgr.handle(alias) else { continue };
            if call_idx < 3 {
                tracing::debug!(call_idx, host = %alias, ports = ?output_host.incoming_port_names(), "daedalus graph: output host ports");
            }
            for port in output_host.incoming_ports() {
                let port_name = port.name();
                let port_lc = port_name.to_ascii_lowercase();
                if !self.host_output_ports_lc.contains(&port_lc) {
                    // The graph JSON contract is authoritative. If the runtime exposes extra
                    // ports (e.g. due to stale persisted graphs or dynamic nodes), drain+ignore.
                    let _ = output_host.drain(port_name);
                    continue;
                }
                let wants_preview = self.preview_ports_lc.contains(&port_lc);
                let wants_image_sample = wants_preview;
                let port_type = port.resolved_type();
                let is_image_type = port_type.map(is_image_payload).unwrap_or(false);
                let typed_image = is_image_type;
                if host_output_debug_enabled() && (call_idx < 3 || call_idx.is_multiple_of(120)) {
                    tracing::info!(
                        target: "helios_engine::graph",
                        port = %port_name,
                        wants_preview,
                        typed_image,
                        resolved_type = ?port_type,
                        "host output port state"
                    );
                }
                if typed_image && !wants_image_sample {
                    popped_outputs += output_host.drain(port_name).len();
                    continue;
                }
                // Keep image samples only for the active preview/output path. Additional image
                // outputs can be surprisingly expensive because they force CPU materialization and
                // then stay resident in `image_samples`.
                let unresolved_type = port_type.is_none();
                if typed_image || unresolved_type {
                    let mut image_popped = false;
                    match port.try_pop::<DynamicImage>() {
                        Ok(Some((_corr, img))) => {
                            popped_outputs += 1;
                            if preview_key.is_none() && wants_preview {
                                preview_key = Some(port_lc.clone());
                            }
                            image_updates.push((port_lc.clone(), img));
                            image_popped = true;
                            if call_idx < 3 && wants_preview {
                                tracing::debug!(call_idx, port = %port_name, "daedalus graph: pulled preview output");
                            }
                        }
                        Ok(None) => {}
                        Err(_err) => {
                            // Many CV nodes in lib-cv use `Payload<DynamicImage>` so they can
                            // run under GPU/CPU affinity without forcing node authors to
                            // manually handle transfers. The host output bridge should be
                            // able to decode those payloads too.
                            match port.try_pop::<Payload<DynamicImage>>() {
                                Ok(Some((_corr, payload))) => match payload {
                                    Payload::Cpu(img) => {
                                        popped_outputs += 1;
                                        if preview_key.is_none() && wants_preview {
                                            preview_key = Some(port_lc.clone());
                                        }
                                        image_updates.push((port_lc.clone(), img));
                                        image_popped = true;
                                    }
                                    Payload::Gpu(handle) => {
                                        if let Some(gpu) = self.gpu.as_ref() {
                                            match <DynamicImage as daedalus::gpu::GpuSendable>::download(&handle, gpu) {
                                                Ok(img) => {
                                                    popped_outputs += 1;
                                                    if preview_key.is_none() && wants_preview {
                                                        preview_key = Some(port_lc.clone());
                                                    }
                                                    image_updates.push((port_lc.clone(), img));
                                                    image_popped = true;
                                                }
                                                Err(err) => {
                                                    if wants_preview || host_output_debug_enabled() {
                                                        tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output GPU image download failed");
                                                    }
                                                }
                                            }
                                        }
                                    }
                                },
                                Ok(None) => {}
                                Err(err) => {
                                    if wants_preview || host_output_debug_enabled() {
                                        tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output image decode failed");
                                    }
                                }
                            }

                            if !image_popped {
                                match port.try_pop::<Payload<GrayImage>>() {
                                    Ok(Some((_corr, payload))) => {
                                        if let Payload::Cpu(gray) = payload {
                                            popped_outputs += 1;
                                            if preview_key.is_none() && wants_preview {
                                                preview_key = Some(port_lc.clone());
                                            }
                                            image_updates.push((port_lc.clone(), DynamicImage::ImageLuma8(gray)));
                                            image_popped = true;
                                            if call_idx < 3 && wants_preview {
                                                tracing::debug!(call_idx, port = %port_name, "daedalus graph: pulled preview output");
                                            }
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(err) => {
                                        if wants_preview || host_output_debug_enabled() {
                                            tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output image decode failed");
                                        }
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

                // Structured outputs: pull as Daedalus `Value` for typed sampling and JSON preview.
                match port.try_pop::<daedalus::data::model::Value>() {
                    Ok(Some((_corr, value))) => {
                        popped_outputs += 1;
                        let port_name = port_lc.clone();
                        if let Some(json) = daedalus_value_to_json(&value) {
                            if call_idx < 3 {
                                tracing::debug!(call_idx, port = %port_name, "daedalus graph: captured json output");
                            }
                            json_updates.push((port_name.clone(), json));
                        }
                        value_updates.push((port_name, value));
                    }
                    result => {
                        // Some nodes emit JSON as `serde_json::Value` or plain `String`; accept both.
                        if let Some((_corr, json)) = port.try_pop_any::<Value>() {
                            popped_outputs += 1;
                            let port_name = port_lc.clone();
                            if call_idx < 3 {
                                tracing::debug!(call_idx, port = %port_name, "daedalus graph: captured typed json output");
                            }
                            value_updates.push((port_name.clone(), json_to_daedalus_value(&json)));
                            json_updates.push((port_name, json));
                            continue;
                        }

                        if let Some((_corr, raw)) = port.try_pop_any::<String>() {
                            popped_outputs += 1;
                            let port_name = port_lc.clone();
                            let parsed = serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw));
                            if call_idx < 3 {
                                tracing::debug!(call_idx, port = %port_name, "daedalus graph: captured string json output");
                            }
                            value_updates.push((port_name.clone(), json_to_daedalus_value(&parsed)));
                            json_updates.push((port_name, parsed));
                            continue;
                        }

                        // Common CV payload: detection lists are emitted as typed vectors, not
                        // necessarily as Daedalus `Value`/JSON. Capture and mirror them so
                        // calibration solve and output sampling can consume the same graph port.
                        if let Some((_corr, detections)) = port.try_pop_any::<Vec<ArucoDetection2D>>() {
                            popped_outputs += 1;
                            let port_name = port_lc.clone();
                            match serde_json::to_value(&detections) {
                                Ok(json) => {
                                    if call_idx < 3 {
                                        tracing::debug!(call_idx, port = %port_name, len = detections.len(), "daedalus graph: captured typed detection output");
                                    }
                                    value_updates.push((port_name.clone(), json_to_daedalus_value(&json)));
                                    json_updates.push((port_name, json));
                                }
                                Err(err) => {
                                    if host_output_debug_enabled() {
                                        tracing::warn!(target: "helios_engine::graph", port = %port_name, error = %err, "failed to serialize typed detection output");
                                    }
                                }
                            }
                            continue;
                        }

                        if host_output_debug_enabled() {
                            if let Err(err) = result {
                                // Many ports are neither image nor value-like; ignore unless debugging.
                                tracing::warn!(target: "helios_engine::graph", port = %port_name, error = ?err, "host output value decode failed");
                            }
                        }
                    }
                }
            }
        }

        if call_idx < 3 {
            tracing::debug!(call_idx, popped_outputs, "daedalus graph: output host pop count");
        }

        if !json_updates.is_empty() {
            if let Ok(mut guard) = self.json_samples.lock() {
                for (port, value) in json_updates {
                    guard.insert(port, value);
                }
            }
        }
        if !value_updates.is_empty() {
            if let Ok(mut guard) = self.value_samples.lock() {
                for (port, value) in value_updates {
                    guard.insert(port, value);
                }
            }
        }
        if !image_updates.is_empty() {
            // Select the preview image locally first so we can still render a frame even if the
            // sample cache lock is poisoned/unavailable.
            if let Some(key) = preview_key.as_deref() {
                if let Some(idx) = image_updates.iter().position(|(port, _)| port == key) {
                    let (_port, img) = image_updates.swap_remove(idx);
                    preview_image = Some(img);
                }
            }
            if let Ok(mut guard) = self.image_samples.lock() {
                for (port, value) in image_updates {
                    guard.insert(port, value);
                }
            }
        }

        if let Some(img) = preview_image {
            return Some(img);
        }
        // If no output port produced a frame, report it and keep the last good preview frame.
        if call_idx < 3 || call_idx.is_multiple_of(120) {
            tracing::warn!(call_idx, "graph produced no output; falling back to last preview");
        }
        if let Ok(mut metrics) = self.metrics.lock() {
            metrics.record_warning(format!("graph output missing: selected ports {:?} produced no frames", self.preview_ports));
        }
        None
    }

    fn pipeline_metrics(&self) -> Option<PipelineGraphMetrics> {
        self.metrics.lock().ok().map(|metrics| {
            let mut snapshot = metrics.snapshot();
            snapshot.sample_cache = sample_cache_metrics(&self.image_samples, &self.json_samples, &self.value_samples);
            annotate_retained_output_metrics(&mut snapshot, &self.host_output_port_owners);
            snapshot
        })
    }

    fn host_output_ports(&self) -> Option<Vec<String>> {
        Some(self.host_output_ports.clone())
    }

    fn host_output_port_types(&self) -> Option<BTreeMap<String, DaedalusTypeExpr>> {
        Some(self.host_output_port_types.clone())
    }

    fn sample_json_output(&self, port: &str) -> Option<Value> {
        let key = port.to_ascii_lowercase();
        self.json_samples.lock().ok()?.get(&key).cloned()
    }

    fn sample_value_output(&self, port: &str) -> Option<DaedalusValue> {
        let key = port.to_ascii_lowercase();
        self.value_samples.lock().ok()?.get(&key).cloned()
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

fn host_output_debug_enabled() -> bool {
    static ENABLED: AtomicU64 = AtomicU64::new(u64::MAX);
    let cached = ENABLED.load(Ordering::Relaxed);
    if cached != u64::MAX {
        return cached == 1;
    }
    let enabled = env::var("HELIOS_HOST_OUTPUT_DEBUG").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
    ENABLED.store(if enabled { 1 } else { 0 }, Ordering::Relaxed);
    enabled
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
    json_samples: &Mutex<BTreeMap<String, Value>>,
    value_samples: &Mutex<BTreeMap<String, DaedalusValue>>,
) -> Option<PipelineSampleCacheMetrics> {
    let image_ports = image_samples.lock().ok().map(|guard| guard.iter().map(|(port, image)| (port.clone(), dynamic_image_size_bytes(image))).collect::<BTreeMap<_, _>>())?;
    let json_ports = json_samples.lock().ok().map(|guard| guard.iter().map(|(port, value)| (port.clone(), json_value_size_bytes(value))).collect::<BTreeMap<_, _>>())?;
    let value_ports = value_samples.lock().ok().map(|guard| guard.iter().map(|(port, value)| (port.clone(), daedalus_value_size_bytes(value))).collect::<BTreeMap<_, _>>())?;

    let metrics = PipelineSampleCacheMetrics {
        image_sample_count: image_ports.len() as u64,
        image_sample_bytes: image_ports.values().copied().sum(),
        json_sample_count: json_ports.len() as u64,
        json_sample_bytes: json_ports.values().copied().sum(),
        value_sample_count: value_ports.len() as u64,
        value_sample_bytes: value_ports.values().copied().sum(),
        image_ports,
        json_ports,
        value_ports,
    };

    if metrics.image_sample_count == 0
        && metrics.json_sample_count == 0
        && metrics.value_sample_count == 0
        && metrics.image_sample_bytes == 0
        && metrics.json_sample_bytes == 0
        && metrics.value_sample_bytes == 0
    {
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
        owners.entry(key).or_insert(from.0);
    }
    owners
}

fn build_demand_sinks(
    plan: &RuntimePlan,
    output_hosts: &[String],
    preview_ports: &[String],
    host_output_ports: &[String],
    host_output_port_types: &BTreeMap<String, DaedalusTypeExpr>,
    demand_driven: bool,
) -> Vec<RuntimeSink> {
    if !demand_driven {
        return Vec::new();
    }

    let selector = if let Some(index) = host_output_sink_node_index(plan, output_hosts) {
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

    let mut sinks = Vec::new();
    for port in sink_ports {
        sinks.push(RuntimeSink { node: selector.clone(), port: Some(port) });
    }
    sinks
}

fn pool_size_from_env() -> Option<usize> {
    env::var("HELIOS_DAEDALUS_POOL_SIZE").ok().and_then(|v| v.parse::<usize>().ok()).filter(|v| *v > 0)
}

fn dedicated_executor_from_env() -> bool {
    env::var("HELIOS_DAEDALUS_DEDICATED_EXECUTOR").ok().map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
}

fn executor_busy_behavior_from_env() -> ExecutorBusyBehavior {
    match env::var("HELIOS_DAEDALUS_EXECUTOR_BUSY").ok().as_deref() {
        Some(raw) if raw.eq_ignore_ascii_case("block") => ExecutorBusyBehavior::Block,
        Some(raw) if raw.eq_ignore_ascii_case("drop") => ExecutorBusyBehavior::Drop,
        Some(raw) if raw.eq_ignore_ascii_case("true") || raw == "1" => ExecutorBusyBehavior::Block,
        _ => ExecutorBusyBehavior::Drop,
    }
}

fn executor_busy_timeout_from_env() -> Option<Duration> {
    env::var("HELIOS_DAEDALUS_EXECUTOR_BUSY_TIMEOUT_MS").ok().and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0).map(Duration::from_millis)
}

fn env_flag(name: &str) -> bool {
    env::var(name).ok().map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")).unwrap_or(false)
}

fn plan_uses_gpu(plan: &RuntimePlan) -> bool {
    plan.segments.iter().any(|segment| !matches!(segment.compute, ComputeAffinity::CpuOnly))
}

fn host_outputs_in_graph_enabled(plan: Option<&RuntimePlan>) -> bool {
    let requested = env_flag("HELIOS_DAEDALUS_HOST_OUTPUTS_IN_GRAPH");
    if !requested {
        return false;
    }
    if plan.is_some_and(plan_uses_gpu) {
        tracing::warn!("HELIOS_DAEDALUS_HOST_OUTPUTS_IN_GRAPH is disabled for GPU plans (stability guard)");
        return false;
    }
    true
}

fn demand_driven_enabled(plan: Option<&RuntimePlan>) -> bool {
    let requested = env_flag("HELIOS_DAEDALUS_DEMAND_DRIVEN");
    if !requested {
        return false;
    }
    if plan.is_some_and(plan_uses_gpu) {
        tracing::warn!("HELIOS_DAEDALUS_DEMAND_DRIVEN is disabled for GPU plans (stability guard)");
        return false;
    }
    true
}

fn perf_counters_enabled_from_env() -> bool {
    cfg!(all(feature = "perf-counters", target_os = "linux")) && env_flag("HELIOS_PERF_COUNTERS")
}

fn pprof_enabled_from_env() -> bool {
    cfg!(feature = "pprof") && env_flag("HELIOS_PPROF")
}

fn pprof_frames_from_env() -> u64 {
    env::var("HELIOS_PPROF_FRAMES").ok().and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0).unwrap_or(1)
}

fn pprof_duration_ms_from_env() -> Option<u64> {
    // Prefer ms if specified, otherwise accept seconds.
    if let Some(ms) = env::var("HELIOS_PPROF_DURATION_MS").ok().and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0) {
        return Some(ms);
    }
    env::var("HELIOS_PPROF_DURATION_SECS").ok().and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0).map(|s| s.saturating_mul(1000))
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
