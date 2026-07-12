use std::{collections::BTreeMap, time::Instant};

use daedalus::data::model::Value;
use daedalus::{
    engine::{Engine, EngineConfig as DaedalusEngineConfig, GpuBackend, HostGraph, RuntimeMode},
    planner::Graph,
    runtime::{BackpressureStrategy, HostBridgeManager, RuntimeNode, RuntimePlan, RuntimeSink, handler_registry::HandlerRegistry, plugins::PluginRegistry},
};
use orion::{
    ResourceId,
    control_plane::{ResourceRecord, StateSnapshot, config_json_value},
};
use styx::imports::framelease::FrameLease;

use crate::{
    config::EngineConfig,
    model::{ExecutionArtifactRecord, ExecutionBinding, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload, GraphRef, LoadedPlugin},
    stream_io,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutionSnapshot {
    pub sessions: Vec<ExecutionSessionState>,
    pub artifacts: Vec<ExecutionArtifactRecord>,
}

type ResidentHostGraph = HostGraph<HandlerRegistry>;

#[derive(Default)]
pub struct ResidentExecutionSet {
    sessions: BTreeMap<String, ResidentExecution>,
}

struct ResidentExecution {
    workload: ExecutionWorkload,
    host_graph: ResidentHostGraph,
    output_host_alias: Option<String>,
    output_ports: Vec<String>,
    output_sinks: Vec<RuntimeSink>,
    last_input_fingerprints: BTreeMap<String, BindingInputFingerprint>,
    planning_elapsed_ms: f64,
    tick_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BindingInputFingerprint {
    Frame { resource_id: String, timestamp: u64, width: u32, height: u32, fourcc: String, payload_bytes: usize },
    Resource { resource_id: String, revision: String },
}

struct PreparedBindingInput {
    input: String,
    fingerprint: BindingInputFingerprint,
    payload: PreparedBindingPayload,
}

enum PreparedBindingPayload {
    Payload(daedalus::transport::Payload),
    AnyString(String),
}

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("graph artifact references are not implemented yet: {0}")]
    UnsupportedArtifactGraph(String),
    #[error("graph resource references are not implemented yet: {0}")]
    UnsupportedResourceGraph(String),
    #[error("failed to parse inline graph JSON: {0}")]
    ParseGraph(#[from] serde_json::Error),
    #[error("failed to build execution engine: {0}")]
    Engine(String),
    #[error("failed to plan graph: {0}")]
    Plan(String),
    #[error("graph requires host bridge input '{0}', but no host alias was found")]
    MissingHostBridge(String),
    #[error("bound resource '{0}' was not found in Orion state")]
    MissingResource(String),
    #[error("bound resource '{0}' has no supported endpoint")]
    MissingResourceEndpoint(String),
    #[error("failed to read bound resource '{resource_id}' from '{path}': {error}")]
    ReadResource { resource_id: String, path: String, error: std::io::Error },
    #[error("frame stream error: {0}")]
    FrameStream(#[from] stream_io::FrameStreamError),
    #[error("failed to execute graph: {0}")]
    Execute(String),
    #[error("workload requires Daedalus plugin '{0}', but it is not loaded")]
    MissingPlugin(String),
    #[error("workload requires Daedalus plugin '{plugin}' version '{required}', but loaded version is {loaded}")]
    PluginVersionMismatch { plugin: String, required: String, loaded: String },
}

pub fn execute_workloads(
    config: &EngineConfig,
    registry: &PluginRegistry,
    host_manager: &HostBridgeManager,
    loaded_plugins: &[LoadedPlugin],
    workloads: &[ExecutionWorkload],
    state_snapshot: Option<&StateSnapshot>,
    observed_at_ms: u64,
) -> ExecutionSnapshot {
    let resources = state_snapshot.map(resources_for_execution).unwrap_or_default();

    let mut sessions = Vec::with_capacity(workloads.len());
    let mut artifacts = Vec::new();

    for workload in workloads {
        match execute_workload(config, registry, host_manager, loaded_plugins, workload, &resources, observed_at_ms) {
            Ok((session, produced)) => {
                sessions.push(session);
                artifacts.extend(produced);
            }
            Err(error) => {
                sessions.push(ExecutionSessionState {
                    workload_id: workload.workload_id.clone(),
                    session_id: session_id_for(workload),
                    status: ExecutionSessionStatus::Failed,
                    observed_at_ms,
                    graph_ref: workload.graph_ref.clone(),
                    bindings: workload.bindings.clone(),
                    plugin_requirements: workload.plugin_requirements.clone(),
                    message: Some(error.to_string()),
                });
            }
        }
    }

    ExecutionSnapshot { sessions, artifacts }
}

impl ResidentExecutionSet {
    pub fn tick_workloads(
        &mut self,
        config: &EngineConfig,
        registry: &PluginRegistry,
        host_manager: &HostBridgeManager,
        loaded_plugins: &[LoadedPlugin],
        workloads: &[ExecutionWorkload],
        state_snapshot: Option<&StateSnapshot>,
        observed_at_ms: u64,
    ) -> ExecutionSnapshot {
        let resources = state_snapshot.map(resources_for_execution).unwrap_or_default();
        let workload_by_id = workloads.iter().map(|workload| (workload.workload_id.as_str(), workload)).collect::<BTreeMap<_, _>>();
        self.sessions.retain(|workload_id, resident| workload_by_id.get(workload_id.as_str()).is_some_and(|workload| resident.workload == **workload));

        let mut sessions = Vec::with_capacity(workloads.len());
        let mut artifacts = Vec::new();

        for workload in workloads {
            if !self.sessions.contains_key(workload.workload_id.as_str()) {
                match build_resident_execution(config, registry, host_manager, loaded_plugins, workload) {
                    Ok(resident) => {
                        self.sessions.insert(workload.workload_id.clone(), resident);
                    }
                    Err(error) => {
                        sessions.push(failed_session(workload, observed_at_ms, error));
                        continue;
                    }
                }
            }

            let Some(resident) = self.sessions.get_mut(workload.workload_id.as_str()) else {
                continue;
            };
            match tick_resident_execution(config, resident, &resources, observed_at_ms) {
                Ok((session, produced)) => {
                    sessions.push(session);
                    artifacts.extend(produced);
                }
                Err(error) => {
                    sessions.push(failed_session(workload, observed_at_ms, error));
                }
            }
        }

        ExecutionSnapshot { sessions, artifacts }
    }
}

fn resources_for_execution(snapshot: &StateSnapshot) -> BTreeMap<ResourceId, ResourceRecord> {
    let mut resources = snapshot.state.desired.resources.clone();
    resources.extend(snapshot.state.observed.resources.clone());
    resources
}

fn execute_workload(
    config: &EngineConfig,
    registry: &PluginRegistry,
    host_manager: &HostBridgeManager,
    loaded_plugins: &[LoadedPlugin],
    workload: &ExecutionWorkload,
    resources: &BTreeMap<ResourceId, ResourceRecord>,
    observed_at_ms: u64,
) -> Result<(ExecutionSessionState, Vec<ExecutionArtifactRecord>), ExecutionError> {
    let started_at = Instant::now();
    validate_plugin_requirements(workload, loaded_plugins)?;
    let engine = Engine::new(daedalus_engine_config(config)).map_err(|error| ExecutionError::Engine(error.to_string()))?;
    let plan_started_at = Instant::now();
    let mut host_graph = compile_host_graph(&engine, registry, host_manager, workload)?;
    let planning_elapsed_ms = plan_started_at.elapsed().as_secs_f64() * 1000.0;
    let output_host_alias = host_output_alias(host_graph.runtime_plan());
    let output_ports = host_output_ports(host_graph.runtime_plan(), output_host_alias.as_deref());
    let output_sinks = output_ports.iter().map(|port| RuntimeSink::node_id("io.host_bridge").port(port.clone())).collect::<Vec<_>>();

    let binding_started_at = Instant::now();
    for binding in &workload.bindings {
        inject_binding(host_graph.host(), binding, resources)?;
    }
    let binding_elapsed_ms = binding_started_at.elapsed().as_secs_f64() * 1000.0;

    let execute_started_at = Instant::now();
    let telemetry = if !workload.bindings.is_empty() && !output_sinks.is_empty() {
        Some(host_graph.tick_selected(output_sinks).map_err(|error| ExecutionError::Execute(error.to_string()))?)
    } else if !workload.bindings.is_empty() {
        host_graph.tick_until_idle().map_err(|error| ExecutionError::Execute(error.to_string()))?
    } else {
        Some(host_graph.run_executor_once().map_err(|error| ExecutionError::Execute(error.to_string()))?.telemetry)
    };
    let execution_elapsed_ms = execute_started_at.elapsed().as_secs_f64() * 1000.0;
    let telemetry_count = telemetry.as_ref().map(|telemetry| telemetry.node_metrics.len().try_into().unwrap_or_default()).unwrap_or_default();

    let artifact_started_at = Instant::now();
    let produced_artifacts = collect_artifacts(
        config,
        workload,
        host_graph.bridge_manager(),
        output_host_alias.as_deref().or(Some("host")),
        &output_ports,
        observed_at_ms,
        telemetry_count,
        planning_elapsed_ms,
        binding_elapsed_ms,
        execution_elapsed_ms,
        started_at.elapsed().as_secs_f64() * 1000.0,
    )?;
    let artifact_elapsed_ms = artifact_started_at.elapsed().as_secs_f64() * 1000.0;
    let total_elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
    let timing_message = serde_json::json!({
        "status": "succeeded",
        "timings_ms": {
            "planning": planning_elapsed_ms,
            "binding_injection": binding_elapsed_ms,
            "execution": execution_elapsed_ms,
            "artifact_collection": artifact_elapsed_ms,
            "total_engine": total_elapsed_ms
        },
        "node_metrics_count": telemetry_count
    })
    .to_string();

    Ok((
        ExecutionSessionState {
            workload_id: workload.workload_id.clone(),
            session_id: session_id_for(workload),
            status: ExecutionSessionStatus::Succeeded,
            observed_at_ms,
            graph_ref: workload.graph_ref.clone(),
            bindings: workload.bindings.clone(),
            plugin_requirements: workload.plugin_requirements.clone(),
            message: Some(timing_message),
        },
        produced_artifacts,
    ))
}

fn build_resident_execution(
    config: &EngineConfig,
    registry: &PluginRegistry,
    host_manager: &HostBridgeManager,
    loaded_plugins: &[LoadedPlugin],
    workload: &ExecutionWorkload,
) -> Result<ResidentExecution, ExecutionError> {
    validate_plugin_requirements(workload, loaded_plugins)?;
    let engine = Engine::new(daedalus_engine_config(config)).map_err(|error| ExecutionError::Engine(error.to_string()))?;
    let plan_started_at = Instant::now();
    let host_graph = compile_host_graph(&engine, registry, host_manager, workload)?;
    let planning_elapsed_ms = plan_started_at.elapsed().as_secs_f64() * 1000.0;
    let output_host_alias = host_output_alias(host_graph.runtime_plan());
    let output_ports = host_output_ports(host_graph.runtime_plan(), output_host_alias.as_deref());
    let output_sinks = output_ports.iter().map(|port| RuntimeSink::node_id("io.host_bridge").port(port.clone())).collect::<Vec<_>>();
    Ok(ResidentExecution { workload: workload.clone(), host_graph, output_host_alias, output_ports, output_sinks, last_input_fingerprints: BTreeMap::new(), planning_elapsed_ms, tick_count: 0 })
}

fn compile_host_graph(engine: &Engine, registry: &PluginRegistry, host_manager: &HostBridgeManager, workload: &ExecutionWorkload) -> Result<ResidentHostGraph, ExecutionError> {
    let graph = graph_for(workload)?;
    engine.compile_host_graph_plugin_registry(registry, graph, registry.handlers(), host_manager.clone(), "host").map_err(|error| ExecutionError::Plan(error.to_string()))
}

fn daedalus_engine_config(config: &EngineConfig) -> DaedalusEngineConfig {
    let mut engine_config = DaedalusEngineConfig::default();
    let _ = config;
    engine_config.gpu = GpuBackend::Cpu;
    engine_config.planner.enable_gpu = false;
    engine_config.runtime.mode = RuntimeMode::Serial;
    engine_config.runtime.backpressure = BackpressureStrategy::None;
    engine_config.runtime.pool_size = None;
    engine_config
}

fn tick_resident_execution(
    config: &EngineConfig,
    resident: &mut ResidentExecution,
    resources: &BTreeMap<ResourceId, ResourceRecord>,
    observed_at_ms: u64,
) -> Result<(ExecutionSessionState, Vec<ExecutionArtifactRecord>), ExecutionError> {
    let started_at = Instant::now();
    let binding_started_at = Instant::now();
    let prepared_inputs = resident.workload.bindings.iter().map(|binding| prepare_binding_input(binding, resources)).collect::<Result<Vec<_>, _>>()?;
    let inputs_changed = prepared_inputs.iter().any(|input| resident.last_input_fingerprints.get(input.input.as_str()) != Some(&input.fingerprint));
    if !prepared_inputs.is_empty() && !inputs_changed {
        let binding_elapsed_ms = binding_started_at.elapsed().as_secs_f64() * 1000.0;
        let message = serde_json::json!({
            "status": "idle",
            "graph_resident": true,
            "skipped_unchanged_inputs": true,
            "tick_count": resident.tick_count,
            "timings_ms": {
                "planning_cached": resident.planning_elapsed_ms,
                "binding_prepare": binding_elapsed_ms,
                "execution": 0.0,
                "artifact_collection": 0.0,
                "total_engine": started_at.elapsed().as_secs_f64() * 1000.0
            },
            "node_metrics_count": 0
        })
        .to_string();
        return Ok((
            ExecutionSessionState {
                workload_id: resident.workload.workload_id.clone(),
                session_id: session_id_for(&resident.workload),
                status: ExecutionSessionStatus::Running,
                observed_at_ms,
                graph_ref: resident.workload.graph_ref.clone(),
                bindings: resident.workload.bindings.clone(),
                plugin_requirements: resident.workload.plugin_requirements.clone(),
                message: Some(message),
            },
            Vec::new(),
        ));
    }
    let input_fingerprints = prepared_inputs.iter().map(|input| (input.input.clone(), input.fingerprint.clone())).collect::<BTreeMap<_, _>>();
    for input in prepared_inputs {
        push_prepared_binding(resident.host_graph.host(), input);
    }
    let binding_elapsed_ms = binding_started_at.elapsed().as_secs_f64() * 1000.0;

    let execute_started_at = Instant::now();
    let telemetry = if !resident.workload.bindings.is_empty() && !resident.output_sinks.is_empty() {
        Some(resident.host_graph.tick_selected(resident.output_sinks.clone()).map_err(|error| ExecutionError::Execute(error.to_string()))?)
    } else if !resident.workload.bindings.is_empty() {
        resident.host_graph.tick_until_idle().map_err(|error| ExecutionError::Execute(error.to_string()))?
    } else {
        Some(resident.host_graph.run_executor_once().map_err(|error| ExecutionError::Execute(error.to_string()))?.telemetry)
    };
    let execution_elapsed_ms = execute_started_at.elapsed().as_secs_f64() * 1000.0;
    let telemetry_count = telemetry.as_ref().map(|telemetry| telemetry.node_metrics.len().try_into().unwrap_or_default()).unwrap_or_default();

    resident.last_input_fingerprints = input_fingerprints;
    resident.tick_count = resident.tick_count.saturating_add(1);
    let artifact_started_at = Instant::now();
    let produced_artifacts = collect_artifacts(
        config,
        &resident.workload,
        resident.host_graph.bridge_manager(),
        resident.output_host_alias.as_deref().or(Some("host")),
        &resident.output_ports,
        observed_at_ms,
        telemetry_count,
        resident.planning_elapsed_ms,
        binding_elapsed_ms,
        execution_elapsed_ms,
        started_at.elapsed().as_secs_f64() * 1000.0,
    )?;
    let artifact_elapsed_ms = artifact_started_at.elapsed().as_secs_f64() * 1000.0;
    let total_elapsed_ms = started_at.elapsed().as_secs_f64() * 1000.0;
    let timing_message = serde_json::json!({
        "status": "running",
        "graph_resident": true,
        "tick_count": resident.tick_count,
        "timings_ms": {
            "planning_cached": resident.planning_elapsed_ms,
            "binding_injection": binding_elapsed_ms,
            "execution": execution_elapsed_ms,
            "artifact_collection": artifact_elapsed_ms,
            "total_engine": total_elapsed_ms
        },
        "node_metrics_count": telemetry_count
    })
    .to_string();

    Ok((
        ExecutionSessionState {
            workload_id: resident.workload.workload_id.clone(),
            session_id: session_id_for(&resident.workload),
            status: ExecutionSessionStatus::Running,
            observed_at_ms,
            graph_ref: resident.workload.graph_ref.clone(),
            bindings: resident.workload.bindings.clone(),
            plugin_requirements: resident.workload.plugin_requirements.clone(),
            message: Some(timing_message),
        },
        produced_artifacts,
    ))
}

fn failed_session(workload: &ExecutionWorkload, observed_at_ms: u64, error: ExecutionError) -> ExecutionSessionState {
    ExecutionSessionState {
        workload_id: workload.workload_id.clone(),
        session_id: session_id_for(workload),
        status: ExecutionSessionStatus::Failed,
        observed_at_ms,
        graph_ref: workload.graph_ref.clone(),
        bindings: workload.bindings.clone(),
        plugin_requirements: workload.plugin_requirements.clone(),
        message: Some(error.to_string()),
    }
}

fn validate_plugin_requirements(workload: &ExecutionWorkload, loaded_plugins: &[LoadedPlugin]) -> Result<(), ExecutionError> {
    for requirement in &workload.plugin_requirements {
        let Some(plugin) = loaded_plugins.iter().find(|plugin| plugin.plugin_name.as_deref() == Some(requirement.plugin_name.as_str())) else {
            return Err(ExecutionError::MissingPlugin(requirement.plugin_name.clone()));
        };
        if let Some(required) = &requirement.version {
            match plugin.plugin_version.as_deref() {
                Some(loaded) if loaded == required => {}
                Some(loaded) => {
                    return Err(ExecutionError::PluginVersionMismatch { plugin: requirement.plugin_name.clone(), required: required.clone(), loaded: loaded.to_string() });
                }
                None => {
                    return Err(ExecutionError::PluginVersionMismatch { plugin: requirement.plugin_name.clone(), required: required.clone(), loaded: "unknown".into() });
                }
            }
        }
    }
    Ok(())
}

fn graph_for(workload: &ExecutionWorkload) -> Result<Graph, ExecutionError> {
    match &workload.graph_ref {
        GraphRef::InlineSpec(graph) => {
            let value: serde_json::Value = serde_json::from_str(graph)?;
            let graph_value = value.get("graph").cloned().unwrap_or(value);
            serde_json::from_value(graph_value).map_err(ExecutionError::from)
        }
        GraphRef::ArtifactId(artifact_id) => Err(ExecutionError::UnsupportedArtifactGraph(artifact_id.clone())),
        GraphRef::ResourceId(resource_id) => Err(ExecutionError::UnsupportedResourceGraph(resource_id.clone())),
    }
}

fn inject_binding(host: &daedalus::runtime::HostBridgeHandle, binding: &ExecutionBinding, resources: &BTreeMap<ResourceId, ResourceRecord>) -> Result<(), ExecutionError> {
    push_prepared_binding(host, prepare_binding_input(binding, resources)?);
    Ok(())
}

fn prepare_binding_input(binding: &ExecutionBinding, resources: &BTreeMap<ResourceId, ResourceRecord>) -> Result<PreparedBindingInput, ExecutionError> {
    let resource = resources.get(binding.resource_id.as_str()).ok_or_else(|| ExecutionError::MissingResource(binding.resource_id.clone()))?;
    if resource.resource_type.as_str() == "stream.channel" {
        let frame = stream_io::import_latest_frame_from_resource_endpoints(&resource.endpoints)?;
        let fingerprint = BindingInputFingerprint::Frame {
            resource_id: binding.resource_id.clone(),
            timestamp: frame.meta().timestamp,
            width: frame.meta().format.resolution.width.into(),
            height: frame.meta().format.resolution.height.into(),
            fourcc: frame.meta().format.code.to_string(),
            payload_bytes: frame.payload_bytes(),
        };
        return Ok(PreparedBindingInput { input: binding.input.clone(), fingerprint, payload: PreparedBindingPayload::Payload(stream_io::framelease_payload(frame)) });
    }
    let payload = serde_json::to_string(&resource_binding_payload(resource)).map_err(|error| ExecutionError::Execute(error.to_string()))?;
    let revision = serde_json::to_string(resource).unwrap_or_else(|_| format!("{resource:?}"));
    Ok(PreparedBindingInput {
        input: binding.input.clone(),
        fingerprint: BindingInputFingerprint::Resource { resource_id: binding.resource_id.clone(), revision },
        payload: PreparedBindingPayload::AnyString(payload),
    })
}

fn push_prepared_binding(host: &daedalus::runtime::HostBridgeHandle, input: PreparedBindingInput) {
    match input.payload {
        PreparedBindingPayload::Payload(payload) => host.push_payload(input.input.as_str(), payload),
        PreparedBindingPayload::AnyString(payload) => host.push_any(input.input.as_str(), payload),
    };
}

fn resource_binding_payload(resource: &ResourceRecord) -> serde_json::Value {
    let action_result = resource.state.as_ref().and_then(|state| state.action_result.as_ref()).map(|result| {
        serde_json::json!({
            "action_kind": result.action_kind,
            "status": format!("{:?}", result.status),
            "data": result
                .data
                .as_ref()
                .and_then(|value| config_json_value(&BTreeMap::from([("value".to_string(), value.clone())])).ok())
                .and_then(|value| value.get("value").cloned())
                .unwrap_or(serde_json::Value::Null),
            "error": result.error,
        })
    });

    serde_json::json!({
        "resource_id": resource.resource_id.as_str(),
        "resource_type": resource.resource_type.as_str(),
        "provider_id": resource.provider_id.as_str(),
        "labels": resource.labels,
        "endpoints": resource.endpoints,
        "lease_state": format!("{:?}", resource.lease_state),
        "state": {
            "observed_at_ms": resource.state.as_ref().map(|state| state.observed_at_ms),
            "action_result": action_result,
            "config": resource.state.as_ref().and_then(|state| state.config.as_ref()).map(resource_config_to_json),
        },
    })
}

fn resource_config_to_json(config: &orion::control_plane::ResourceConfigState) -> serde_json::Value {
    config_json_value(&config.payload).unwrap_or_else(|error| {
        serde_json::json!({
            "decode_error": error.to_string(),
        })
    })
}

fn collect_artifacts(
    config: &EngineConfig,
    workload: &ExecutionWorkload,
    host_manager: &HostBridgeManager,
    host_alias: Option<&str>,
    output_ports: &[String],
    observed_at_ms: u64,
    telemetry_count: u64,
    planning_elapsed_ms: f64,
    binding_elapsed_ms: f64,
    execution_elapsed_ms: f64,
    elapsed_before_artifacts_ms: f64,
) -> Result<Vec<ExecutionArtifactRecord>, ExecutionError> {
    let Some(host) = host_alias.and_then(|alias| host_manager.handle(alias)).or_else(|| host_manager.handle("host")) else {
        return Ok(Vec::new());
    };

    let mut artifacts = Vec::new();
    for port in output_ports {
        while let Some(payload) = host.try_pop_payload(port) {
            let endpoints = frame_output_endpoints(config, workload, port, &payload)?;
            artifacts.push(ExecutionArtifactRecord {
                workload_id: workload.workload_id.clone(),
                session_id: session_id_for(workload),
                artifact_id: format!("{}.{}", session_id_for(workload), sanitize_id_component(port)),
                kind: if endpoints.is_empty() { format!("host_output:{port}") } else { format!("stream.channel:{port}") },
                observed_at_ms,
                message: Some(format_host_output_payload(&payload)),
                endpoints,
            });
        }
    }

    artifacts.push(ExecutionArtifactRecord {
        workload_id: workload.workload_id.clone(),
        session_id: session_id_for(workload),
        artifact_id: format!("{}.telemetry", session_id_for(workload)),
        kind: "execution.telemetry".into(),
        observed_at_ms,
        message: Some(
            serde_json::json!({
                "node_metrics_count": telemetry_count,
                "timings_ms": {
                    "planning": planning_elapsed_ms,
                    "binding_injection": binding_elapsed_ms,
                    "execution": execution_elapsed_ms,
                    "before_artifact_collection_total": elapsed_before_artifacts_ms
                }
            })
            .to_string(),
        ),
        endpoints: Vec::new(),
    });

    Ok(artifacts)
}

fn frame_output_endpoints(config: &EngineConfig, workload: &ExecutionWorkload, port: &str, payload: &daedalus::transport::Payload) -> Result<Vec<String>, ExecutionError> {
    let Some(frame) = payload.get_ref::<FrameLease>() else {
        return Ok(Vec::new());
    };
    let stream_id = format!("{}.{}", session_id_for(workload), sanitize_id_component(port));
    let (_path, endpoints) = stream_io::publish_output_frame(&config.stream_dir, &stream_id, frame)?;
    Ok(endpoints)
}

fn format_host_output_payload(payload: &daedalus::transport::Payload) -> String {
    if let Some(frame) = payload.get_ref::<FrameLease>() {
        return serde_json::json!({
            "type": "styx.framelease",
            "width": frame.meta().format.resolution.width,
            "height": frame.meta().format.resolution.height,
            "fourcc": frame.meta().format.code.to_string(),
            "timestamp": frame.meta().timestamp,
            "payload_bytes": frame.payload_bytes(),
            "residency": format!("{:?}", frame.residency()),
        })
        .to_string();
    }
    if let Some(value) = payload.get_ref::<Value>() {
        return format_value_payload(value.clone());
    }
    if let Some(value) = payload.get_ref::<String>() {
        return value.clone();
    }
    if let Some(value) = payload.get_ref::<f64>() {
        return serde_json::json!({ "value": value }).to_string();
    }
    if let Some(value) = payload.get_ref::<i64>() {
        return serde_json::json!({ "value": value }).to_string();
    }
    if let Some(value) = payload.get_ref::<bool>() {
        return serde_json::json!({ "value": value }).to_string();
    }
    serde_json::json!({
        "type_key": payload.type_key().to_string(),
        "rust_type": payload.storage_rust_type_name(),
    })
    .to_string()
}

fn host_output_alias(plan: &RuntimePlan) -> Option<String> {
    host_bridge_alias(plan, |idx, _node| plan.edges.iter().any(|edge| edge.to().0 == idx))
}

fn host_bridge_alias(plan: &RuntimePlan, predicate: impl Fn(usize, &RuntimeNode) -> bool) -> Option<String> {
    plan.nodes
        .iter()
        .enumerate()
        .find(|(idx, node)| matches!(node.metadata.get("host_bridge"), Some(Value::Bool(true))) && predicate(*idx, node))
        .map(|(_, node)| node.label.as_deref().unwrap_or(node.id.as_str()).to_ascii_lowercase())
}

fn host_output_ports(plan: &RuntimePlan, host_alias: Option<&str>) -> Vec<String> {
    let Some(host_index) = plan.nodes.iter().enumerate().find_map(|(idx, node)| {
        let is_host = matches!(node.metadata.get("host_bridge"), Some(Value::Bool(true)));
        let alias_matches = host_alias.map(|alias| node.label.as_deref().unwrap_or(node.id.as_str()).eq_ignore_ascii_case(alias)).unwrap_or(true);
        (is_host && alias_matches).then_some(idx)
    }) else {
        return Vec::new();
    };

    plan.edges.iter().filter(|edge| edge.to().0 == host_index).map(|edge| edge.target_port().to_string()).collect()
}

fn format_value_payload(value: Value) -> String {
    match value {
        Value::Unit => serde_json::json!({ "value": null }).to_string(),
        Value::Bool(value) => serde_json::json!({ "value": value }).to_string(),
        Value::Int(value) => serde_json::json!({ "value": value }).to_string(),
        Value::Float(value) => serde_json::json!({ "value": value }).to_string(),
        Value::String(value) => value.into_owned(),
        other => format!("{other:?}"),
    }
}

fn sanitize_id_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '_',
        })
        .collect()
}

fn session_id_for(workload: &ExecutionWorkload) -> String {
    format!("session.{}", workload.workload_id)
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU32, str::FromStr};

    use super::*;
    use daedalus::{
        PluginRegistry, declare_plugin,
        host_bridge::host_port,
        install_host_bridge,
        macros::node,
        runtime::{NodeError, plugins::RegistryPluginExt},
    };
    use orion::control_plane::{ResourceActionResult, ResourceActionStatus, ResourceRecord, ResourceState, TypedConfigValue};
    use styx::{
        core::prelude::{BufferPool, ColorSpace, FourCc, FrameMeta, MediaFormat, Resolution, plane_layout_from_dims},
        imports::framelease::FrameLease,
    };

    #[node(id = "test.source", outputs("out"))]
    fn source() -> Result<i64, NodeError> {
        Ok(7)
    }

    #[node(id = "test.echo", inputs("inp"), outputs("out"))]
    fn echo(inp: String) -> Result<String, NodeError> {
        Ok(inp)
    }

    #[node(id = "test.frame_passthrough", inputs("frame"), outputs("frame"))]
    fn frame_passthrough(frame: FrameLease) -> Result<FrameLease, NodeError> {
        Ok(frame)
    }

    #[node(id = "test.frame_context", inputs("frame", "context"), outputs("out"))]
    fn frame_context(frame: FrameLease, context: String) -> Result<String, NodeError> {
        Ok(serde_json::json!({
            "timestamp": frame.meta().timestamp,
            "context": context,
        })
        .to_string())
    }

    declare_plugin!(EngineTestPlugin, "engine.test", [source, echo, frame_passthrough, frame_context]);

    fn test_frame(timestamp: u64) -> FrameLease {
        let format = MediaFormat::new(FourCc::from_str("RG24").expect("fourcc"), Resolution::new(2, 2).expect("resolution"), ColorSpace::Srgb);
        let layout = plane_layout_from_dims(NonZeroU32::new(2).expect("width"), NonZeroU32::new(2).expect("height"), 3);
        let pool = BufferPool::lazy(layout.len, 1);
        FrameLease::single_plane(FrameMeta::new(format, timestamp), pool.lease(), layout.len, layout.stride)
    }

    #[test]
    fn execute_inline_graph_produces_output_artifact() {
        let plugin = EngineTestPlugin::new();
        let mut plugins = PluginRegistry::new();
        plugins.install_plugin(&plugin).expect("install plugin");
        let host_manager = HostBridgeManager::new();
        install_host_bridge(&mut plugins, host_manager.clone()).expect("install host bridge");
        let src = plugin.source.clone().alias("src");
        let graph = plugins.graph_builder().expect("graph builder").host_bridge("host").node(&src).connect(&src.outputs.out, &host_port("host", "result")).build();
        let graph_json = serde_json::to_string(&graph).expect("serialize graph");

        let snapshot = execute_workloads(
            &EngineConfig::default(),
            &plugins,
            &host_manager,
            &[LoadedPlugin { path: "<test>".into(), plugin_name: Some("engine.test".into()), plugin_version: None, abi_version: None }],
            &[ExecutionWorkload {
                workload_id: "workload.inline".into(),
                artifact_id: "artifact.inline".into(),
                assigned_node_id: "node-local".into(),
                graph_ref: GraphRef::InlineSpec(graph_json),
                bindings: Vec::new(),
                plugin_requirements: Vec::new(),
            }],
            None,
            42,
        );

        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Succeeded);
        assert!(snapshot.artifacts.iter().any(|artifact| artifact.message.as_deref().is_some_and(|message| message.contains("\"value\":7"))));
    }

    #[test]
    fn execute_inline_graph_can_consume_orion_resource_state_binding() {
        let plugin = EngineTestPlugin::new();
        let mut plugins = PluginRegistry::new();
        plugins.install_plugin(&plugin).expect("install plugin");
        let host_manager = HostBridgeManager::new();
        install_host_bridge(&mut plugins, host_manager.clone()).expect("install host bridge");
        let echo_node = plugin.echo.clone().alias("echo");
        let graph = plugins
            .graph_builder()
            .expect("graph builder")
            .host_bridge("host")
            .node(&echo_node)
            .connect(&host_port("host", "sensor"), &echo_node.inputs.inp)
            .connect(&echo_node.outputs.out, &host_port("host", "result"))
            .build();
        let graph_json = serde_json::to_string(&graph).expect("serialize graph");

        let mut resources = BTreeMap::new();
        resources.insert(
            ResourceId::new("gpio_line.node-local.0"),
            ResourceRecord::builder("gpio_line.node-local.0", "gpio.line", "provider.peripherals.node-local")
                .state(ResourceState::new(42).with_action_result(ResourceActionResult {
                    action_kind: "gpio.read".into(),
                    status: ResourceActionStatus::Read,
                    data: Some(TypedConfigValue::Bool(true)),
                    error: None,
                }))
                .build(),
        );
        let state_snapshot = StateSnapshot {
            state: orion::control_plane::ClusterStateEnvelope {
                desired: orion::control_plane::DesiredClusterState { resources, ..Default::default() },
                observed: Default::default(),
                applied: Default::default(),
            },
        };

        let snapshot = execute_workloads(
            &EngineConfig::default(),
            &plugins,
            &host_manager,
            &[LoadedPlugin { path: "<test>".into(), plugin_name: Some("engine.test".into()), plugin_version: None, abi_version: None }],
            &[ExecutionWorkload {
                workload_id: "workload.bound.state".into(),
                artifact_id: "artifact.bound.state".into(),
                assigned_node_id: "node-local".into(),
                graph_ref: GraphRef::InlineSpec(graph_json),
                bindings: vec![ExecutionBinding { input: "sensor".into(), resource_id: "gpio_line.node-local.0".into(), node_id: "node-local".into() }],
                plugin_requirements: Vec::new(),
            }],
            Some(&state_snapshot),
            100,
        );

        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Succeeded);
        assert!(snapshot.artifacts.iter().any(|artifact| artifact.message.as_deref().is_some_and(|message| message.contains("gpio.read"))));
    }

    #[test]
    fn execute_inline_graph_passes_frame_stream_through_daedalus() {
        let plugin = EngineTestPlugin::new();
        let mut plugins = PluginRegistry::new();
        plugins.install_plugin(&plugin).expect("install plugin");
        let host_manager = HostBridgeManager::new();
        install_host_bridge(&mut plugins, host_manager.clone()).expect("install host bridge");

        let temp = tempfile::tempdir().expect("tempdir");
        let input_frame = test_frame(314);
        let (_input_path, input_endpoints) = stream_io::publish_output_frame(temp.path(), "input-frame", &input_frame).expect("publish input frame");

        let pass = plugin.frame_passthrough.clone().alias("pass");
        let graph = plugins
            .graph_builder()
            .expect("graph builder")
            .host_bridge("host")
            .node(&pass)
            .connect(&host_port("host", "camera"), &pass.inputs.frame)
            .connect(&pass.outputs.frame, &host_port("host", "processed"))
            .build();
        let graph_json = serde_json::to_string(&graph).expect("serialize graph");

        let stream_resource_id = ResourceId::new("stream.channel.camera.front.raw");
        let mut stream_resource = ResourceRecord::builder(stream_resource_id.clone(), "stream.channel", "provider.peripherals.node-local").build();
        stream_resource.endpoints = input_endpoints;
        let state_snapshot = StateSnapshot {
            state: orion::control_plane::ClusterStateEnvelope {
                desired: Default::default(),
                observed: orion::control_plane::ObservedClusterState { resources: BTreeMap::from([(stream_resource_id.clone(), stream_resource)]), ..Default::default() },
                applied: Default::default(),
            },
        };
        let config = EngineConfig { stream_dir: temp.path().join("engine-output"), ..EngineConfig::default() };

        let snapshot = execute_workloads(
            &config,
            &plugins,
            &host_manager,
            &[LoadedPlugin { path: "<test>".into(), plugin_name: Some("engine.test".into()), plugin_version: None, abi_version: None }],
            &[ExecutionWorkload {
                workload_id: "workload.frame-pass".into(),
                artifact_id: "artifact.frame-pass".into(),
                assigned_node_id: "node-local".into(),
                graph_ref: GraphRef::InlineSpec(graph_json),
                bindings: vec![ExecutionBinding { input: "camera".into(), resource_id: stream_resource_id.as_str().to_string(), node_id: "node-local".into() }],
                plugin_requirements: Vec::new(),
            }],
            Some(&state_snapshot),
            500,
        );

        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Succeeded);
        let artifact = snapshot.artifacts.iter().find(|artifact| artifact.kind == "stream.channel:processed").expect("frame output artifact");
        let output_frame = stream_io::import_latest_frame_from_resource_endpoints(&artifact.endpoints).expect("import output frame");
        assert_eq!(output_frame.meta().timestamp, 314);
        assert_eq!(output_frame.meta().format.code.to_string(), "RG24");
    }

    #[test]
    fn resident_execution_ticks_same_graph_across_frame_updates() {
        let plugin = EngineTestPlugin::new();
        let mut plugins = PluginRegistry::new();
        plugins.install_plugin(&plugin).expect("install plugin");
        let host_manager = HostBridgeManager::new();
        install_host_bridge(&mut plugins, host_manager.clone()).expect("install host bridge");

        let temp = tempfile::tempdir().expect("tempdir");
        let input_frame = test_frame(314);
        let (_input_path, input_endpoints) = stream_io::publish_output_frame(temp.path(), "resident-input-frame", &input_frame).expect("publish input frame");

        let pass = plugin.frame_passthrough.clone().alias("resident-pass");
        let graph = plugins
            .graph_builder()
            .expect("graph builder")
            .host_bridge("host")
            .node(&pass)
            .connect(&host_port("host", "camera"), &pass.inputs.frame)
            .connect(&pass.outputs.frame, &host_port("host", "processed"))
            .build();
        let graph_json = serde_json::to_string(&graph).expect("serialize graph");

        let stream_resource_id = ResourceId::new("stream.channel.camera.resident.raw");
        let mut stream_resource = ResourceRecord::builder(stream_resource_id.clone(), "stream.channel", "provider.peripherals.node-local").build();
        stream_resource.endpoints = input_endpoints;
        let state_snapshot = StateSnapshot {
            state: orion::control_plane::ClusterStateEnvelope {
                desired: Default::default(),
                observed: orion::control_plane::ObservedClusterState { resources: BTreeMap::from([(stream_resource_id.clone(), stream_resource)]), ..Default::default() },
                applied: Default::default(),
            },
        };
        let config = EngineConfig { stream_dir: temp.path().join("resident-engine-output"), ..EngineConfig::default() };
        let workload = ExecutionWorkload {
            workload_id: "workload.resident-frame-pass".into(),
            artifact_id: "artifact.resident-frame-pass".into(),
            assigned_node_id: "node-local".into(),
            graph_ref: GraphRef::InlineSpec(graph_json),
            bindings: vec![ExecutionBinding { input: "camera".into(), resource_id: stream_resource_id.as_str().to_string(), node_id: "node-local".into() }],
            plugin_requirements: Vec::new(),
        };
        let loaded_plugins = vec![LoadedPlugin { path: "<test>".into(), plugin_name: Some("engine.test".into()), plugin_version: None, abi_version: None }];
        let mut resident = ResidentExecutionSet::default();

        let first = resident.tick_workloads(&config, &plugins, &host_manager, &loaded_plugins, std::slice::from_ref(&workload), Some(&state_snapshot), 500);
        assert_eq!(first.sessions[0].status, ExecutionSessionStatus::Running);
        assert!(first.sessions[0].message.as_deref().is_some_and(|message| message.contains("\"tick_count\":1")));
        let first_artifact = first.artifacts.iter().find(|artifact| artifact.kind == "stream.channel:processed").expect("first frame output artifact");
        let first_output_frame = stream_io::import_latest_frame_from_resource_endpoints(&first_artifact.endpoints).expect("import first output frame");
        assert_eq!(first_output_frame.meta().timestamp, 314);

        let updated_frame = test_frame(628);
        let (_input_path, _input_endpoints) = stream_io::publish_output_frame(temp.path(), "resident-input-frame", &updated_frame).expect("publish updated input frame");
        let second = resident.tick_workloads(&config, &plugins, &host_manager, &loaded_plugins, std::slice::from_ref(&workload), Some(&state_snapshot), 750);
        assert_eq!(second.sessions[0].status, ExecutionSessionStatus::Running);
        assert!(second.sessions[0].message.as_deref().is_some_and(|message| message.contains("\"tick_count\":2")));
        let second_artifact = second.artifacts.iter().find(|artifact| artifact.kind == "stream.channel:processed").expect("second frame output artifact");
        let second_output_frame = stream_io::import_latest_frame_from_resource_endpoints(&second_artifact.endpoints).expect("import second output frame");
        assert_eq!(second_output_frame.meta().timestamp, 628);

        let idle = resident.tick_workloads(&config, &plugins, &host_manager, &loaded_plugins, std::slice::from_ref(&workload), Some(&state_snapshot), 1000);
        assert_eq!(idle.sessions[0].status, ExecutionSessionStatus::Running);
        assert!(idle.sessions[0].message.as_deref().is_some_and(|message| message.contains("\"skipped_unchanged_inputs\":true")));
        assert!(idle.sessions[0].message.as_deref().is_some_and(|message| message.contains("\"tick_count\":2")));
        assert!(idle.artifacts.is_empty());
    }

    #[test]
    fn resident_execution_refeeds_all_inputs_when_one_binding_changes() {
        let plugin = EngineTestPlugin::new();
        let mut plugins = PluginRegistry::new();
        plugins.install_plugin(&plugin).expect("install plugin");
        let host_manager = HostBridgeManager::new();
        install_host_bridge(&mut plugins, host_manager.clone()).expect("install host bridge");

        let temp = tempfile::tempdir().expect("tempdir");
        let input_frame = test_frame(900);
        let (_input_path, input_endpoints) = stream_io::publish_output_frame(temp.path(), "fusion-input-frame", &input_frame).expect("publish input frame");

        let fusion = plugin.frame_context.clone().alias("fusion");
        let graph = plugins
            .graph_builder()
            .expect("graph builder")
            .host_bridge("host")
            .node(&fusion)
            .connect(&host_port("host", "camera"), &fusion.inputs.frame)
            .connect(&host_port("host", "imu"), &fusion.inputs.context)
            .connect(&fusion.outputs.out, &host_port("host", "result"))
            .build();
        let graph_json = serde_json::to_string(&graph).expect("serialize graph");

        let stream_resource_id = ResourceId::new("stream.channel.camera.fusion.raw");
        let mut stream_resource = ResourceRecord::builder(stream_resource_id.clone(), "stream.channel", "provider.peripherals.node-local").build();
        stream_resource.endpoints = input_endpoints;
        let imu_resource_id = ResourceId::new("imu.node-local.primary");
        let imu_resource = ResourceRecord::builder(imu_resource_id.clone(), "imu.sensor", "provider.peripherals.node-local")
            .state(ResourceState::new(100).with_action_result(ResourceActionResult {
                action_kind: "imu.read".into(),
                status: ResourceActionStatus::Read,
                data: Some(TypedConfigValue::String("imu-a".into())),
                error: None,
            }))
            .build();
        let mut resources = BTreeMap::from([(stream_resource_id.clone(), stream_resource), (imu_resource_id.clone(), imu_resource)]);
        let mut state_snapshot = StateSnapshot {
            state: orion::control_plane::ClusterStateEnvelope {
                desired: Default::default(),
                observed: orion::control_plane::ObservedClusterState { resources: resources.clone(), ..Default::default() },
                applied: Default::default(),
            },
        };
        let config = EngineConfig { stream_dir: temp.path().join("fusion-engine-output"), ..EngineConfig::default() };
        let workload = ExecutionWorkload {
            workload_id: "workload.fusion".into(),
            artifact_id: "artifact.fusion".into(),
            assigned_node_id: "node-local".into(),
            graph_ref: GraphRef::InlineSpec(graph_json),
            bindings: vec![
                ExecutionBinding { input: "camera".into(), resource_id: stream_resource_id.as_str().to_string(), node_id: "node-local".into() },
                ExecutionBinding { input: "imu".into(), resource_id: imu_resource_id.as_str().to_string(), node_id: "node-local".into() },
            ],
            plugin_requirements: Vec::new(),
        };
        let loaded_plugins = vec![LoadedPlugin { path: "<test>".into(), plugin_name: Some("engine.test".into()), plugin_version: None, abi_version: None }];
        let mut resident = ResidentExecutionSet::default();

        let first = resident.tick_workloads(&config, &plugins, &host_manager, &loaded_plugins, std::slice::from_ref(&workload), Some(&state_snapshot), 100);
        assert!(first.artifacts.iter().any(|artifact| artifact.message.as_deref().is_some_and(|message| message.contains("imu-a") && message.contains("\"timestamp\":900"))));

        let imu_resource = ResourceRecord::builder(imu_resource_id.clone(), "imu.sensor", "provider.peripherals.node-local")
            .state(ResourceState::new(200).with_action_result(ResourceActionResult {
                action_kind: "imu.read".into(),
                status: ResourceActionStatus::Read,
                data: Some(TypedConfigValue::String("imu-b".into())),
                error: None,
            }))
            .build();
        resources.insert(imu_resource_id.clone(), imu_resource);
        state_snapshot.state.observed.resources = resources;

        let second = resident.tick_workloads(&config, &plugins, &host_manager, &loaded_plugins, std::slice::from_ref(&workload), Some(&state_snapshot), 200);
        assert!(second.artifacts.iter().any(|artifact| artifact.message.as_deref().is_some_and(|message| message.contains("imu-b") && message.contains("\"timestamp\":900"))));
        assert!(second.sessions[0].message.as_deref().is_some_and(|message| message.contains("\"tick_count\":2")));
    }

    #[test]
    fn execute_workload_fails_when_required_plugin_is_missing() {
        let plugin = EngineTestPlugin::new();
        let mut plugins = PluginRegistry::new();
        plugins.install_plugin(&plugin).expect("install plugin");
        let host_manager = HostBridgeManager::new();
        install_host_bridge(&mut plugins, host_manager.clone()).expect("install host bridge");
        let src = plugin.source.clone().alias("src");
        let graph = plugins.graph_builder().expect("graph builder").host_bridge("host").node(&src).connect(&src.outputs.out, &host_port("host", "result")).build();
        let graph_json = serde_json::to_string(&graph).expect("serialize graph");

        let snapshot = execute_workloads(
            &EngineConfig::default(),
            &plugins,
            &host_manager,
            &[],
            &[ExecutionWorkload {
                workload_id: "workload.missing-plugin".into(),
                artifact_id: "artifact.inline".into(),
                assigned_node_id: "node-local".into(),
                graph_ref: GraphRef::InlineSpec(graph_json),
                bindings: Vec::new(),
                plugin_requirements: vec![crate::model::PluginRequirement { plugin_name: "engine.test".into(), version: None }],
            }],
            None,
            42,
        );

        assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Failed);
        assert!(snapshot.sessions[0].message.as_deref().is_some_and(|message| message.contains("not loaded")));
    }
}
