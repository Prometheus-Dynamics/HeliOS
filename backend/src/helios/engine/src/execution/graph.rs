//! Workload graphs: versioned Daedalus `GraphDocument`s compiled into host-driven graphs.

use std::time::Instant;

use daedalus::{
    data::model::Value,
    engine::{DEFAULT_FRAME_OVERHEAD_WINDOW, Engine, EngineConfig as DaedalusEngineConfig, GpuBackend, HostGraph, MetricsLevel, RuntimeMode},
    planner::{GraphDocument, HostInputPolicy},
    runtime::{BackpressureStrategy, HostBridgeManager, RuntimeEdgeExplanation, handler_registry::HandlerRegistry, plugins::PluginRegistry},
};

use super::ExecutionError;
use crate::model::{ExecutionWorkload, GraphRef, LoadedPlugin};

pub(crate) type ResidentHostGraph = HostGraph<HandlerRegistry>;

/// Host alias used when a graph's host bridge node has no label.
const DEFAULT_HOST_ALIAS: &str = "host";

/// Format tag of the plan introspection published for each session.
pub(crate) const PLAN_FORMAT: &str = "helios.engine.plan.v1";

/// How workload graphs are compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphSettings {
    pub metrics_level: MetricsLevel,
}

impl Default for GraphSettings {
    fn default() -> Self {
        Self { metrics_level: MetricsLevel::Off }
    }
}

/// A workload's graph, compiled once and kept resident while the workload is unchanged.
pub(crate) struct CompiledWorkloadGraph {
    pub host_graph: ResidentHostGraph,
    pub input_ports: Vec<String>,
    pub output_ports: Vec<String>,
    pub planning_ms: f64,
    /// Host ports, the runtime plan and its adapter steps (`PLAN_FORMAT`), for the control
    /// interface.
    pub plan: serde_json::Value,
    pub metrics_level: MetricsLevel,
}

/// Validate plugin requirements, parse the workload's `GraphDocument` and compile it against
/// `registry` with a host bridge of its own, so workloads never share host ports.
///
/// `frame_inputs` are the host inputs camera frames go into; when there are any, every other
/// host input is context and is declared held in the document before planning
/// (`GraphDocument::set_host_input_policy`), so the planner can branch it for consumers that
/// take it by value (a runtime-only `set_held_input` cannot).
pub(crate) fn compile_workload_graph(
    registry: &PluginRegistry,
    loaded_plugins: &[LoadedPlugin],
    workload: &ExecutionWorkload,
    frame_inputs: &[String],
    settings: GraphSettings,
) -> Result<CompiledWorkloadGraph, ExecutionError> {
    validate_plugin_requirements(workload, loaded_plugins)?;
    let mut document = graph_document_for(workload)?;
    validate_document_requires(registry, &document)?;
    let host_alias = host_alias(&document);
    if !frame_inputs.is_empty() {
        declare_context_held(&mut document, &host_alias, frame_inputs)?;
    }
    let requires = serde_json::to_value(&document.requires).unwrap_or_default();
    let engine = Engine::new(daedalus_engine_config(settings)).map_err(|error| ExecutionError::Engine(error.to_string()))?;
    let started_at = Instant::now();
    let host_graph = engine.compile_document_host_graph(registry, document, registry.handlers(), HostBridgeManager::new(), host_alias).map_err(|error| ExecutionError::Plan(error.to_string()))?;
    let planning_ms = started_at.elapsed().as_secs_f64() * 1000.0;
    let input_ports = host_graph.host_inputs().iter().map(|port| port.name().to_string()).collect();
    let output_ports = host_graph.host_outputs().iter().map(|port| port.name().to_string()).collect();
    let plan = plan_introspection(&host_graph, requires, settings.metrics_level);
    Ok(CompiledWorkloadGraph { host_graph, input_ports, output_ports, planning_ms, plan, metrics_level: settings.metrics_level })
}

/// What the UI and API need to show a compiled graph: its host ports with their types, the
/// runtime plan (`explain_plan()`: nodes, edges, policies, handoffs) and, separately, every edge
/// the planner inserted adapters on, so conversions and device transfers are visible.
pub(crate) fn plan_introspection(host_graph: &ResidentHostGraph, requires: serde_json::Value, metrics_level: MetricsLevel) -> serde_json::Value {
    let explanation = host_graph.explain_plan();
    let label = |index: usize| explanation.nodes.get(index).map(|node| node.label.clone().unwrap_or_else(|| node.id.clone())).unwrap_or_default();
    let edge_json = |edge: &RuntimeEdgeExplanation| {
        serde_json::json!({
            "edge": edge.index,
            "from": format!("{}.{}", label(edge.from_node), edge.from_port),
            "to": format!("{}.{}", label(edge.to_node), edge.to_port),
            "steps": edge.adapter_steps,
            "handoff": edge.handoff,
            "reason": edge.handoff_reason,
            "copies_frame": edge.copies_frame,
            "crosses_residency": edge.crosses_residency,
        })
    };
    let edges_where = |keep: fn(&RuntimeEdgeExplanation) -> bool| explanation.edges.iter().filter(|edge| keep(edge)).map(edge_json).collect::<Vec<_>>();
    let adapter_edges = edges_where(|edge| !edge.adapter_steps.is_empty());
    // Edges whose adapters copy a frame or move it between residencies (CPU, GPU, dma-buf):
    // what to look at first when a frame path is slower than it should be.
    let copying_edges = edges_where(|edge| edge.copies_frame);
    let crossing_edges = edges_where(|edge| edge.crosses_residency);
    serde_json::json!({
        "format": PLAN_FORMAT,
        "host_alias": host_graph.host_alias(),
        "requires": requires,
        "metrics_level": format!("{metrics_level:?}"),
        "frame_overhead": host_graph.frame_overhead_enabled(),
        "host_inputs": host_graph.host_inputs(),
        "host_outputs": host_graph.host_outputs(),
        "adapter_edges": adapter_edges,
        "copying_edges": copying_edges,
        "crossing_edges": crossing_edges,
        "plan": explanation,
    })
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

/// A document's `requires` must name every installed plugin that provides one of its nodes (what
/// `PluginRegistry::graph_document` fills in), so stored graphs say what they need. Whether the
/// required plugins are installed is checked when the document is compiled.
fn validate_document_requires(registry: &PluginRegistry, document: &GraphDocument) -> Result<(), ExecutionError> {
    let missing = registry.graph_requirements(&document.graph).into_iter().filter(|needed| !document.requires.iter().any(|listed| listed.id == needed.id)).map(|needed| needed.id).collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }
    Err(ExecutionError::GraphDocument(format!("`requires` does not list plugin(s) {} that provide its nodes; build documents with PluginRegistry::graph_document", missing.join(", "))))
}

/// Declare every host input of `document`'s host bridge `host` except `frame_inputs` held, with
/// Daedalus's `GraphDocument::set_host_input_policy` (kept in the document, as
/// `GraphBuilder::held_input` records it).
pub(super) fn declare_context_held(document: &mut GraphDocument, host: &str, frame_inputs: &[String]) -> Result<(), ExecutionError> {
    let Ok(ports) = document.graph.host_input_ports(host) else {
        // No host bridge: nothing to hold.
        return Ok(());
    };
    let context = ports.into_iter().filter(|port| !frame_inputs.iter().any(|frame| frame == port)).map(str::to_string).collect::<Vec<_>>();
    for port in context {
        document.set_host_input_policy(host, &port, HostInputPolicy::Held).map_err(|error| ExecutionError::Plan(format!("host input '{port}': {error}")))?;
    }
    Ok(())
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

/// Serial execution on the workload's own driver thread: no worker pool, no GPU. With metrics on,
/// Daedalus also records each tick's frame-path overhead (`HostGraph::frame_overhead`).
fn daedalus_engine_config(settings: GraphSettings) -> DaedalusEngineConfig {
    let mut engine_config = DaedalusEngineConfig { gpu: GpuBackend::Cpu, ..DaedalusEngineConfig::default() }.with_metrics_level(settings.metrics_level);
    if settings.metrics_level != MetricsLevel::Off {
        engine_config = engine_config.with_frame_overhead(DEFAULT_FRAME_OVERHEAD_WINDOW);
    }
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
