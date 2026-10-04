//! Workload graphs: versioned Daedalus `GraphDocument`s compiled into host-driven graphs.

use std::time::Instant;

use daedalus::{
    data::model::Value,
    engine::{Engine, EngineConfig as DaedalusEngineConfig, GpuBackend, HostGraph, RuntimeMode},
    planner::GraphDocument,
    runtime::{BackpressureStrategy, HostBridgeManager, handler_registry::HandlerRegistry, plugins::PluginRegistry},
};

use super::ExecutionError;
use crate::{
    model::{ExecutionWorkload, GraphRef, LoadedPlugin},
    stream_io,
};

pub(crate) type ResidentHostGraph = HostGraph<HandlerRegistry>;

/// Host alias used when a graph's host bridge node has no label.
const DEFAULT_HOST_ALIAS: &str = "host";

/// A workload's graph, compiled once and kept resident while the workload is unchanged.
pub(crate) struct CompiledWorkloadGraph {
    pub host_graph: ResidentHostGraph,
    pub output_ports: Vec<String>,
    pub planning_ms: f64,
}

/// Validate plugin requirements, parse the workload's `GraphDocument` and compile it against
/// `registry` with a host bridge of its own, so workloads never share host ports.
pub(crate) fn compile_workload_graph(registry: &PluginRegistry, loaded_plugins: &[LoadedPlugin], workload: &ExecutionWorkload) -> Result<CompiledWorkloadGraph, ExecutionError> {
    validate_plugin_requirements(workload, loaded_plugins)?;
    stream_io::register_framelease_type();
    let document = graph_document_for(workload)?;
    let host_alias = host_alias(&document);
    let engine = Engine::new(daedalus_engine_config()).map_err(|error| ExecutionError::Engine(error.to_string()))?;
    let started_at = Instant::now();
    let host_graph = engine.compile_document_host_graph(registry, document, registry.handlers(), HostBridgeManager::new(), host_alias).map_err(|error| ExecutionError::Plan(error.to_string()))?;
    let planning_ms = started_at.elapsed().as_secs_f64() * 1000.0;
    let output_ports = host_graph.host_outputs().iter().map(|port| port.name().to_string()).collect();
    Ok(CompiledWorkloadGraph { host_graph, output_ports, planning_ms })
}

/// Workload graphs are versioned `GraphDocument`s (`format: "daedalus.graph"`); bare graph JSON
/// is rejected.
fn graph_document_for(workload: &ExecutionWorkload) -> Result<GraphDocument, ExecutionError> {
    match &workload.graph_ref {
        GraphRef::InlineSpec(json) => GraphDocument::from_json(json).map_err(|error| ExecutionError::GraphDocument(error.to_string())),
        GraphRef::ArtifactId(artifact_id) => Err(ExecutionError::UnsupportedArtifactGraph(artifact_id.clone())),
        GraphRef::ResourceId(resource_id) => Err(ExecutionError::UnsupportedResourceGraph(resource_id.clone())),
    }
}

/// The alias of the document's host bridge node (its label, else its id).
fn host_alias(document: &GraphDocument) -> String {
    document
        .graph
        .nodes
        .iter()
        .find(|node| matches!(node.metadata.get("host_bridge"), Some(Value::Bool(true))))
        .map(|node| node.label.clone().unwrap_or_else(|| node.id.to_string()))
        .unwrap_or_else(|| DEFAULT_HOST_ALIAS.to_string())
}

fn daedalus_engine_config() -> DaedalusEngineConfig {
    let mut engine_config = DaedalusEngineConfig { gpu: GpuBackend::Cpu, ..DaedalusEngineConfig::default() };
    engine_config.planner.enable_gpu = false;
    engine_config.runtime.mode = RuntimeMode::Serial;
    engine_config.runtime.backpressure = BackpressureStrategy::None;
    engine_config.runtime.pool_size = None;
    engine_config
}

fn validate_plugin_requirements(workload: &ExecutionWorkload, loaded_plugins: &[LoadedPlugin]) -> Result<(), ExecutionError> {
    for requirement in &workload.plugin_requirements {
        let Some(plugin) = loaded_plugins.iter().find(|plugin| plugin.plugin_name.as_deref() == Some(requirement.plugin_name.as_str())) else {
            return Err(ExecutionError::MissingPlugin(requirement.plugin_name.clone()));
        };
        if let Some(required) = &requirement.version {
            match plugin.plugin_version.as_deref() {
                Some(loaded) if loaded == required => {}
                loaded => {
                    return Err(ExecutionError::PluginVersionMismatch { plugin: requirement.plugin_name.clone(), required: required.clone(), loaded: loaded.unwrap_or("unknown").to_string() });
                }
            }
        }
    }
    Ok(())
}
