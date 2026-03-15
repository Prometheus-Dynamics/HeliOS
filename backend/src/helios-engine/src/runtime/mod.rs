use crate::ipc::{EngineCommand, EngineErrorCode, EngineEvent};
use crate::services::EngineServices;
use crate::stream::read_latest_frame_async;
use std::sync::OnceLock;
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

use crate::daedalus_registry::build_daedalus_runtime_registry;

pub struct EngineRuntime {
    pub services: EngineServices,
    node_registry_snapshot: RwLock<Option<crate::ipc::NodeRegistrySnapshot>>,
}

const START_STREAM_TIMEOUT: Duration = Duration::from_secs(60);
// Stopping a stream must fully release capture + codec resources; this can take longer than a
// couple seconds when an encoder is mid-frame. Keep this generous so we don't return early and
// leak worker threads / hold devices busy across restarts.
const STOP_STREAM_TIMEOUT: Duration = Duration::from_secs(30);
const METRICS_TIMEOUT: Duration = Duration::from_secs(3);

fn env_flag_enabled(var: &str, default_value: bool) -> bool {
    let raw = match std::env::var(var) {
        Ok(v) => v,
        Err(_) => return default_value,
    };
    let v = raw.trim().to_ascii_lowercase();
    if v.is_empty() {
        return default_value;
    }
    matches!(v.as_str(), "1" | "true" | "yes" | "y" | "on" | "enabled")
}

fn cache_node_registry_snapshot_enabled() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| env_flag_enabled("HELIOS_ENGINE_CACHE_NODE_REGISTRY", false))
}

fn calibration_solve_timeout() -> Duration {
    const DEFAULT_SECS: u64 = 300;
    const MIN_SECS: u64 = 30;
    const MAX_SECS: u64 = 1800;
    let secs = std::env::var("HELIOS_CALIBRATION_SOLVE_TIMEOUT_SECS").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_SECS).clamp(MIN_SECS, MAX_SECS);
    Duration::from_secs(secs)
}

impl EngineRuntime {
    pub fn new() -> Self {
        Self { services: EngineServices::new(), node_registry_snapshot: RwLock::new(None) }
    }

    pub async fn subscribe_encoded(&self, stream_id: uuid::Uuid) -> crate::error::Result<tokio::sync::broadcast::Receiver<crate::stream::EncodedFrame>> {
        self.services.subscribe_encoded(stream_id).await
    }

    pub async fn latest_encoded_frame(&self, stream_id: Uuid) -> crate::error::Result<Vec<u8>> {
        read_latest_frame_async(stream_id).await
    }

    pub async fn handle_command(&self, command: EngineCommand) -> EngineEvent {
        match command {
            EngineCommand::List { command_id } => {
                let streams = self.services.list_streams().await;
                EngineEvent::StreamList { command_id, streams }
            }
            EngineCommand::Start { command_id, manifest } => match timeout(START_STREAM_TIMEOUT, self.services.start_stream(*manifest)).await {
                Ok(Ok((stream_id, descriptor))) => EngineEvent::Started { command_id, stream_id, descriptor },
                Ok(Err(err)) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
                Err(_) => EngineEvent::Nack { command_id, code: EngineErrorCode::Timeout, reason: format!("start stream timed out after {}s", START_STREAM_TIMEOUT.as_secs()) },
            },
            EngineCommand::SetCodecs { command_id, stream_id, decoder_id, encoder_id } => match self.services.set_codecs(stream_id, decoder_id, encoder_id).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::SetCalibration { command_id, stream_id, calibration } => match self.services.set_calibration(stream_id, calibration).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::SetCalibrationMode { command_id, stream_id, enabled, dictionary, mode } => match self.services.set_calibration_mode(stream_id, enabled, dictionary, mode).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::SolveCalibration { command_id, request } => {
                let timeout_budget = calibration_solve_timeout();
                match timeout(timeout_budget, self.services.solve_calibration(request)).await {
                    Ok(Ok(response)) => EngineEvent::CalibrationSolved { command_id, response },
                    Ok(Err(err)) => {
                        let (code, reason) = match err {
                            crate::services::calibration::CalibrationSolveFailure::InvalidInput(reason) => (EngineErrorCode::InvalidInput, reason),
                            crate::services::calibration::CalibrationSolveFailure::NotFound(reason) => (EngineErrorCode::NotFound, reason),
                            crate::services::calibration::CalibrationSolveFailure::Internal(reason) => (EngineErrorCode::Internal, reason),
                        };
                        EngineEvent::Nack { command_id, code, reason }
                    }
                    Err(_) => EngineEvent::Nack { command_id, code: EngineErrorCode::Timeout, reason: format!("calibration solve timed out after {}s", timeout_budget.as_secs()) },
                }
            }
            EngineCommand::SolveLocalization { command_id, request } => {
                let request: crate::ipc::LocalizationSolveRequest = match serde_json::from_value(request.into()) {
                    Ok(request) => request,
                    Err(err) => {
                        return EngineEvent::Nack { command_id, code: EngineErrorCode::InvalidInput, reason: format!("invalid localization solve request: {err}") };
                    }
                };
                match solve_localization_request(request).await {
                    Ok(response) => match serde_json::to_value(response) {
                        Ok(response) => EngineEvent::LocalizationSolved { command_id, response: crate::ipc::JsonWire(response) },
                        Err(err) => EngineEvent::Nack { command_id, code: EngineErrorCode::Internal, reason: format!("failed to encode localization solve response: {err}") },
                    },
                    Err(reason) => EngineEvent::Nack { command_id, code: EngineErrorCode::InvalidInput, reason },
                }
            }
            EngineCommand::GetLocalizationPipelineStatus { command_id, request } => {
                let request: crate::ipc::LocalizationPipelineStatusRequest = match serde_json::from_value(request.into()) {
                    Ok(request) => request,
                    Err(err) => {
                        return EngineEvent::Nack { command_id, code: EngineErrorCode::InvalidInput, reason: format!("invalid localization pipeline status request: {err}") };
                    }
                };
                let response = crate::localization::pipeline::status(&request.profile_id).await;
                match serde_json::to_value(response) {
                    Ok(response) => EngineEvent::LocalizationPipelineStatus { command_id, response: crate::ipc::JsonWire(response) },
                    Err(err) => EngineEvent::Nack { command_id, code: EngineErrorCode::Internal, reason: format!("failed to encode localization pipeline status: {err}") },
                }
            }
            EngineCommand::ListLocalizationPipelineOutputs { command_id, request } => {
                let request: crate::ipc::LocalizationPipelineGraphRequest = match serde_json::from_value(request.into()) {
                    Ok(request) => request,
                    Err(err) => {
                        return EngineEvent::Nack { command_id, code: EngineErrorCode::InvalidInput, reason: format!("invalid localization pipeline outputs request: {err}") };
                    }
                };
                let graph = localization_pipeline_graph_from_request(&request);
                match crate::localization::pipeline::list_outputs(&request.profile, &graph).await {
                    Ok(outputs) => EngineEvent::LocalizationPipelineOutputs { command_id, outputs },
                    Err(reason) => EngineEvent::Nack { command_id, code: EngineErrorCode::InvalidInput, reason },
                }
            }
            EngineCommand::SampleLocalizationPipelineOutput { command_id, request } => {
                let request: crate::ipc::LocalizationPipelineSampleRequest = match serde_json::from_value(request.into()) {
                    Ok(request) => request,
                    Err(err) => {
                        return EngineEvent::Nack { command_id, code: EngineErrorCode::InvalidInput, reason: format!("invalid localization pipeline sample request: {err}") };
                    }
                };
                let graph = localization_pipeline_sample_graph_from_request(&request);
                let fetcher = localization_source_fetcher_from_values(request.source_values.clone());
                match crate::localization::pipeline::sample_output(&fetcher, &request.profile, &request.sources, &graph, &request.output_key).await {
                    Ok(response) => match serde_json::to_value(response) {
                        Ok(response) => EngineEvent::LocalizationPipelineOutputSample { command_id, response: crate::ipc::JsonWire(response) },
                        Err(err) => EngineEvent::Nack { command_id, code: EngineErrorCode::Internal, reason: format!("failed to encode localization pipeline sample: {err}") },
                    },
                    Err(reason) => EngineEvent::Nack { command_id, code: localization_pipeline_error_code(&reason), reason },
                }
            }
            EngineCommand::Stop { command_id, stream_id } => match timeout(STOP_STREAM_TIMEOUT, self.services.stop_stream(stream_id)).await {
                Ok(Ok(_)) => EngineEvent::Stopped { command_id, stream_id },
                Ok(Err(err)) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
                Err(_) => EngineEvent::Nack { command_id, code: EngineErrorCode::Timeout, reason: format!("stop stream timed out after {}s", STOP_STREAM_TIMEOUT.as_secs()) },
            },
            EngineCommand::SetControl { command_id, stream_id, control_id, value } => match self.services.set_control(stream_id, control_id, value).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::GetControls { command_id, stream_id } => match self.services.get_controls(stream_id).await {
                Ok(controls) => EngineEvent::Controls { command_id, stream_id, controls },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::GetMetrics { command_id, stream_id } => match timeout(METRICS_TIMEOUT, self.services.get_metrics(stream_id)).await {
                Ok(Ok(metrics)) => EngineEvent::Metrics { command_id, stream_id, metrics },
                Ok(Err(err)) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
                Err(_) => EngineEvent::Nack { command_id, code: EngineErrorCode::Timeout, reason: format!("get metrics timed out after {}s", METRICS_TIMEOUT.as_secs()) },
            },
            EngineCommand::SnapshotJpeg { command_id, stream_id, quality, source } => match timeout(Duration::from_secs(3), self.services.snapshot_jpeg(stream_id, quality, source)).await {
                Ok(Ok(bytes)) => EngineEvent::SnapshotJpeg { command_id, stream_id, quality, bytes },
                Ok(Err(err)) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
                Err(_) => EngineEvent::Nack { command_id, code: EngineErrorCode::Timeout, reason: "snapshot timed out".into() },
            },
            EngineCommand::GetNodeRegistry { command_id } => {
                if cache_node_registry_snapshot_enabled() {
                    let cached = self.node_registry_snapshot.read().await.clone();
                    if let Some(snapshot) = cached {
                        return EngineEvent::NodeRegistry { command_id, snapshot };
                    }
                }

                match build_node_registry_snapshot() {
                    Ok(snapshot) => {
                        if cache_node_registry_snapshot_enabled() {
                            *self.node_registry_snapshot.write().await = Some(snapshot.clone());
                        }
                        EngineEvent::NodeRegistry { command_id, snapshot }
                    }
                    Err(err) => EngineEvent::Nack { command_id, code: EngineErrorCode::Internal, reason: format!("failed to build daedalus registry: {err}") },
                }
            }
            EngineCommand::DiscoverDevices { command_id } => match tokio::task::spawn_blocking(crate::capture::discover_devices_with_errors).await {
                Ok(discovery) => EngineEvent::Discovery { command_id, discovery },
                Err(err) => EngineEvent::Nack { command_id, code: EngineErrorCode::Internal, reason: format!("device discovery task failed: {err}") },
            },
            EngineCommand::RefreshNodeRegistry { command_id } => match build_node_registry_snapshot() {
                Ok(snapshot) => {
                    if cache_node_registry_snapshot_enabled() {
                        *self.node_registry_snapshot.write().await = Some(snapshot.clone());
                    } else {
                        *self.node_registry_snapshot.write().await = None;
                    }
                    EngineEvent::NodeRegistry { command_id, snapshot }
                }
                Err(err) => EngineEvent::Nack { command_id, code: EngineErrorCode::Internal, reason: format!("failed to build daedalus registry: {err}") },
            },
            EngineCommand::ValidateGraph { command_id, graph, active_features, enable_lints } => {
                let mut parsed: daedalus::planner::Graph = match serde_json::from_value(graph.into()) {
                    Ok(graph) => graph,
                    Err(err) => {
                        return EngineEvent::Nack { command_id, code: EngineErrorCode::InvalidInput, reason: format!("invalid daedalus graph: {err}") };
                    }
                };

                let host_mgr = daedalus::runtime::host_bridge::HostBridgeManager::new();
                let built = match build_daedalus_runtime_registry(&host_mgr, Some(&parsed)) {
                    Ok(built) => built,
                    Err(err) => {
                        return EngineEvent::Nack { command_id, code: EngineErrorCode::Internal, reason: format!("failed to build daedalus registry: {err}") };
                    }
                };

                let (registry, handlers, _plugins) = built.into_parts();
                enforce_registry_default_compute_affinity(&mut parsed, &registry.registry);
                let planner_enable_gpu = planner_enable_gpu_for_validation(&parsed);
                let config = daedalus::planner::PlannerConfig {
                    enable_gpu: planner_enable_gpu,
                    enable_lints,
                    active_features,
                    // Helios persists node port lists as part of the graph contract; validate
                    // them strictly so stale graphs are flagged immediately.
                    strict_port_declarations: true,
                    gpu_caps: None,
                };

                let output = daedalus::planner::build_plan(daedalus::planner::PlannerInput { graph: parsed, registry: &registry.registry }, config);
                let daedalus::planner::PlannerOutput { plan, diagnostics: planner_diagnostics } = output;

                let ok = planner_diagnostics.iter().all(|d| matches!(d.code, daedalus::planner::DiagnosticCode::LintWarning));
                let diagnostics = planner_diagnostics
                    .into_iter()
                    .map(|diag| crate::ipc::PlannerDiagnostic {
                        code: format!("{:?}", diag.code),
                        message: diag.message,
                        span: crate::ipc::PlannerDiagnosticSpan { pass: diag.span.pass, node: diag.span.node, port: diag.span.port },
                    })
                    .collect();
                let (gpu_segments, gpu_edges) = plan.graph.gpu_buffers();
                let node_ids = plan.graph.nodes.into_iter().map(|node| node.id.0).collect();
                let gpu_segments =
                    gpu_segments.into_iter().map(|segment| crate::ipc::GraphGpuSegment { buffer_id: segment.buffer_id, nodes: segment.nodes.into_iter().map(|node| node.0).collect() }).collect();
                let gpu_edges =
                    gpu_edges.into_iter().map(|edge| crate::ipc::GraphGpuEdgeBufferInfo { edge_index: edge.edge_index, gpu_fast_path: edge.gpu_fast_path, buffer_id: edge.buffer_id }).collect();

                drop(handlers);
                EngineEvent::GraphValidation { command_id, report: crate::ipc::GraphValidationReport { ok, diagnostics, gpu_segments, gpu_edges, node_ids } }
            }
            EngineCommand::SetGraph { command_id, stream_id, graph, pipeline_id, output } => match self.services.set_graph(stream_id, graph.into(), pipeline_id, output).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::SetGraphPatch { command_id, stream_id, patch, pipeline_id } => match self.services.apply_graph_patch(stream_id, patch.into(), pipeline_id).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::SetGraphOutput { command_id, stream_id, output } => match self.services.set_graph_output(stream_id, output).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::SetPipelineInputs { command_id, stream_id, pipeline_id, inputs } => {
                let inputs = inputs.into_iter().map(|(key, value)| (key, value.map(|wire| wire.0))).collect();
                match self.services.set_pipeline_inputs(stream_id, pipeline_id, inputs).await {
                    Ok(_) => EngineEvent::Ack { command_id, ok: true },
                    Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
                }
            }
            EngineCommand::ListGraphOutputs { command_id, stream_id } => match self.services.list_graph_outputs(stream_id).await {
                Ok(outputs) => EngineEvent::GraphOutputs { command_id, stream_id, outputs },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::GetGraphOutputSample { command_id, stream_id, port } => match self.services.get_graph_output_sample(stream_id, port.clone()).await {
                Ok(value) => EngineEvent::GraphOutputSample { command_id, stream_id, port, value: crate::ipc::JsonWire(value) },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::SetPipelineLayout { command_id, stream_id, layout } => match self.services.set_pipeline_layout(stream_id, layout).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::SetPipelineWires { command_id, stream_id, wires } => match self.services.set_pipeline_wires(stream_id, wires).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::SetGraphPerf { command_id, stream_id, pipeline_id, enabled } => match self.services.set_graph_perf(stream_id, pipeline_id, enabled).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::ResetGraphMetrics { command_id, stream_id, pipeline_id } => match self.services.reset_graph_metrics(stream_id, pipeline_id).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::CaptureGraphFlamegraph { command_id, stream_id, pipeline_id, duration_ms } => match self.services.capture_graph_flamegraph(stream_id, pipeline_id, duration_ms).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::StartRecording { command_id, stream_id, source, output_path, container, codec, duration_ms, settings } => {
                let params = crate::services::manager::StartRecordingParams { source, output_path, container, codec, duration_ms, settings };
                match self.services.start_recording(stream_id, params).await {
                    Ok(_) => EngineEvent::Ack { command_id, ok: true },
                    Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
                }
            }
            EngineCommand::StopRecording { command_id, stream_id } => match self.services.stop_recording(stream_id).await {
                Ok(_) => EngineEvent::Ack { command_id, ok: true },
                Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
            },
            EngineCommand::CaptureShadowRecording { command_id, stream_id, output_path, container, window_ms } => {
                match self.services.capture_shadow_recording(stream_id, output_path, container, window_ms).await {
                    Ok(_) => EngineEvent::Ack { command_id, ok: true },
                    Err(err) => EngineEvent::Nack { command_id, code: error_code_for(&err), reason: err.to_string() },
                }
            }
        }
    }
}

struct ProvidedLocalizationSourceFetcher {
    values: std::collections::HashMap<String, Result<serde_json::Value, String>>,
}

impl crate::localization::fetch::LocalizationSourceFetcher for ProvidedLocalizationSourceFetcher {
    fn fetch_source_value<'a>(
        &'a self,
        source: &'a crate::localization::config::LocalizationSourceConfig,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send + 'a>> {
        Box::pin(async move { self.values.get(&source.id).cloned().unwrap_or_else(|| Err(format!("missing source value for '{}'", source.id))) })
    }
}

fn localization_source_fetcher_from_values(source_values: Vec<crate::ipc::LocalizationSolveSourceValue>) -> ProvidedLocalizationSourceFetcher {
    ProvidedLocalizationSourceFetcher {
        values: source_values
            .into_iter()
            .map(|source| {
                (
                    source.source_id,
                    match (source.value, source.error) {
                        (Some(value), _) => Ok(value.into()),
                        (None, Some(error)) => Err(error),
                        (None, None) => Err("source value unavailable".to_string()),
                    },
                )
            })
            .collect(),
    }
}

fn localization_pipeline_graph_from_request(request: &crate::ipc::LocalizationPipelineGraphRequest) -> crate::localization::pipeline::LoadedPipelineGraph {
    crate::localization::pipeline::LoadedPipelineGraph { graph: request.graph.clone().into(), graph_updated_at_ms: request.graph_updated_at_ms, template_mtime_ms: request.template_mtime_ms }
}

fn localization_pipeline_sample_graph_from_request(request: &crate::ipc::LocalizationPipelineSampleRequest) -> crate::localization::pipeline::LoadedPipelineGraph {
    crate::localization::pipeline::LoadedPipelineGraph { graph: request.graph.clone().into(), graph_updated_at_ms: request.graph_updated_at_ms, template_mtime_ms: request.template_mtime_ms }
}

fn localization_pipeline_error_code(reason: &str) -> EngineErrorCode {
    if reason == "output sample not available" {
        EngineErrorCode::NotFound
    } else {
        EngineErrorCode::InvalidInput
    }
}

async fn solve_localization_request(request: crate::ipc::LocalizationSolveRequest) -> Result<crate::localization::types::LocalizationSolveResponse, String> {
    let crate::ipc::LocalizationSolveRequest { profile, sources, rig_poses, field_map, calibrations, source_values, apply_field_origin } = request;

    let calibrations = calibrations
        .into_iter()
        .map(|(stream_id, calibration)| {
            (
                stream_id,
                lib_cv::modules::aruco::pose::TagPoseCalibration {
                    fx: calibration.fx,
                    fy: calibration.fy,
                    cx: calibration.cx,
                    cy: calibration.cy,
                    k1: calibration.k1,
                    k2: calibration.k2,
                    p1: calibration.p1,
                    p2: calibration.p2,
                    k3: calibration.k3,
                    undistort_iters: calibration.undistort_iters.clamp(0, u8::MAX as i64) as u8,
                    lens_model: calibration.lens_model,
                },
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let rig_poses = rig_poses.into_iter().map(|(camera_uid, pose)| (camera_uid, crate::localization::math::pose_to_transform(&pose))).collect::<std::collections::HashMap<_, _>>();
    let fetcher = localization_source_fetcher_from_values(source_values);

    Ok(crate::localization::solve::solve_localization(&profile, &sources, &rig_poses, field_map.as_ref(), &calibrations, &fetcher, apply_field_origin).await)
}

fn enforce_registry_default_compute_affinity(graph: &mut daedalus::planner::Graph, registry: &daedalus::registry::store::Registry) {
    let view = registry.view();
    for node in &mut graph.nodes {
        let Some(desc) = view.nodes.get(&daedalus::registry::ids::NodeId(node.id.0.clone())) else {
            continue;
        };
        // Node descriptors own compute affinity; graph JSON values are treated as non-authoritative.
        node.compute = desc.default_compute;
    }
}

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn planner_enable_gpu_for_validation(graph: &daedalus::planner::Graph) -> bool {
    let mut enable_gpu = true;
    if let Some(daedalus::data::model::Value::String(value)) = graph.metadata.get("helios.daedalus.planner.enable_gpu") {
        if let Some(flag) = parse_bool(value.as_ref()) {
            enable_gpu = flag;
        }
    }
    if let Some(daedalus::data::model::Value::String(value)) = graph.metadata.get("helios.daedalus.gpu_backend") {
        match value.trim().to_ascii_lowercase().as_str() {
            "cpu" => enable_gpu = false,
            "gpu" | "device" | "mock" | "gpu-mock" => enable_gpu = true,
            _ => {}
        }
    }
    enable_gpu
}

pub fn build_node_registry_snapshot() -> Result<crate::ipc::NodeRegistrySnapshot, String> {
    let host_mgr = daedalus::runtime::host_bridge::HostBridgeManager::new();
    let built = build_daedalus_runtime_registry(&host_mgr, None).map_err(|err| err.to_string())?;

    // Drop handler closures before unloading plugin libraries.
    let (registry, handlers, plugins) = built.into_parts();
    let view = registry.registry.view();
    let mut nodes = Vec::new();
    for (id, desc) in view.nodes {
        let plugin = id.0.split(':').next().map(|v| v.to_string());
        let input_ports: Vec<crate::ipc::NodeRegistryPort> = desc
            .inputs
            .into_iter()
            .map(|p| crate::ipc::NodeRegistryPort {
                name: p.name,
                ty: crate::ipc::JsonWire(serde_json::to_value(p.ty).unwrap_or(serde_json::Value::Null)),
                source: p.source,
                const_value: p.const_value.map(|value| crate::ipc::JsonWire(serde_json::to_value(value).unwrap_or(serde_json::Value::Null))),
            })
            .collect();
        let output_ports: Vec<crate::ipc::NodeRegistryPort> = desc
            .outputs
            .into_iter()
            .map(|p| crate::ipc::NodeRegistryPort {
                name: p.name,
                ty: crate::ipc::JsonWire(serde_json::to_value(p.ty).unwrap_or(serde_json::Value::Null)),
                source: p.source,
                const_value: p.const_value.map(|value| crate::ipc::JsonWire(serde_json::to_value(value).unwrap_or(serde_json::Value::Null))),
            })
            .collect();
        let fanin_inputs: Vec<crate::ipc::NodeRegistryFanInPort> = desc
            .fanin_inputs
            .into_iter()
            .map(|fanin| crate::ipc::NodeRegistryFanInPort { prefix: fanin.prefix, start: fanin.start, ty: crate::ipc::JsonWire(serde_json::to_value(fanin.ty).unwrap_or(serde_json::Value::Null)) })
            .collect();
        let inputs = input_ports.iter().map(|p| p.name.clone()).collect();
        let outputs = output_ports.iter().map(|p| p.name.clone()).collect();
        let sync_groups = desc
            .sync_groups
            .into_iter()
            .map(|group| crate::ipc::NodeSyncGroup {
                name: group.name,
                policy: format!("{:?}", group.policy),
                ports: group.ports,
                capacity: group.capacity,
                backpressure: group.backpressure.map(|v| format!("{:?}", v)),
            })
            .collect();
        let mut metadata = std::collections::BTreeMap::new();
        for (key, value) in desc.metadata {
            metadata.insert(key, crate::ipc::JsonWire(serde_json::to_value(value).unwrap_or(serde_json::Value::Null)));
        }
        nodes.push(crate::ipc::NodeRegistryNode {
            id: id.0,
            label: desc.label,
            plugin,
            feature_flags: desc.feature_flags,
            sync_groups,
            inputs,
            outputs,
            input_ports,
            fanin_inputs,
            output_ports,
            default_compute: format!("{:?}", desc.default_compute),
            metadata,
        });
    }
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    let types = daedalus::data::typing::snapshot_by_rust_name()
        .into_iter()
        .map(|entry| crate::ipc::TypeRegistryEntry { rust: entry.rust, ty: crate::ipc::JsonWire(serde_json::to_value(entry.expr).unwrap_or(serde_json::Value::Null)) })
        .collect();

    let plugin_compatibility = crate::daedalus_registry::plugin_diagnostics();
    drop(handlers);
    Ok(crate::ipc::NodeRegistrySnapshot { plugins, nodes, types, plugin_compatibility })
}

fn error_code_for(err: &crate::error::Error) -> EngineErrorCode {
    match err {
        crate::error::Error::Unimplemented(_) => EngineErrorCode::Unimplemented,
        crate::error::Error::InvalidState(_) | crate::error::Error::InvalidStateOwned(_) => EngineErrorCode::InvalidState,
        crate::error::Error::NotFound(_) => EngineErrorCode::NotFound,
        crate::error::Error::Conflict(_) => EngineErrorCode::Conflict,
        crate::error::Error::Timeout => EngineErrorCode::Timeout,
    }
}

impl Default for EngineRuntime {
    fn default() -> Self {
        Self::new()
    }
}
