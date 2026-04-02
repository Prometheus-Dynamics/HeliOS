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
mod executor_build;
mod executor_process;
mod flamegraph;
mod helpers;
mod metrics_state;
mod multiplex;
mod payload;
mod perf;
mod policy;
mod roi;
mod runtime_json;
#[cfg(test)]
mod tests;

use self::daedalus_config::{apply_daedalus_engine_config_overrides, apply_daedalus_engine_env_overrides, KEY_RUNTIME_BACKPRESSURE, KEY_RUNTIME_DEFAULT_POLICY};
use self::helpers::*;
use self::metrics_state::*;
use self::payload::{
    decode_runtime_value_as_aruco_detections, decode_runtime_value_fallback, graph_prefers_grayscale_input, is_aruco_detections_payload, is_image_payload, node_requires_color_input,
    preview_port_accepts_grayscale_input,
};
use self::roi::{bootstrap_auto_target_roi_rect, daedalus_value_as_bool, host_output_detection_source_port, is_roi_port, manual_roi_override_active, update_auto_target_roi_state, AutoTargetRoiState};
use self::runtime_json::*;
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
