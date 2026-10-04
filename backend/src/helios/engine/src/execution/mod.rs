//! Resident execution of assigned workloads.
//!
//! Each workload's `GraphDocument` is compiled once and kept until the workload changes. A
//! workload bound to a camera (a resource with a `styx-frames+unix://` endpoint) is
//! frame-driven: a thread of its own feeds every frame to the graph ([`frame_driver`]). Other
//! workloads are ticked on the engine's execution interval when their bound resources change
//! ([`polled`]).

mod bindings;
mod frame_driver;
mod frame_source;
mod graph;
mod outputs;
mod polled;
mod stats;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use daedalus::runtime::plugins::PluginRegistry;
use orion::{
    ResourceId,
    control_plane::{ResourceRecord, StateSnapshot},
};

pub use bindings::STYX_FRAMES_ENDPOINT_SCHEME;

use crate::model::{ExecutionArtifactRecord, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload, LoadedPlugin};
use bindings::resolve_bindings;
use frame_driver::FrameDrivenExecution;
use graph::compile_workload_graph;
use outputs::session_id_for;
use polled::PolledExecution;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutionSnapshot {
    pub sessions: Vec<ExecutionSessionState>,
    pub artifacts: Vec<ExecutionArtifactRecord>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("graph artifact references are not implemented yet: {0}")]
    UnsupportedArtifactGraph(String),
    #[error("graph resource references are not implemented yet: {0}")]
    UnsupportedResourceGraph(String),
    #[error("graph.inline is not a valid Daedalus GraphDocument (format \"daedalus.graph\"): {0}")]
    GraphDocument(String),
    #[error("failed to build execution engine: {0}")]
    Engine(String),
    #[error("failed to plan graph: {0}")]
    Plan(String),
    #[error("bound resource '{0}' was not found in Orion state")]
    MissingResource(String),
    #[error("failed to execute graph: {0}")]
    Execute(String),
    #[error("workload requires Daedalus plugin '{0}', but it is not loaded")]
    MissingPlugin(String),
    #[error("workload requires Daedalus plugin '{plugin}' version '{required}', but loaded version is {loaded}")]
    PluginVersionMismatch { plugin: String, required: String, loaded: String },
}

/// The plugin registry and plugin metadata a workload is compiled against.
pub struct ExecutionPlugins<'a> {
    pub registry: &'a PluginRegistry,
    pub loaded_plugins: &'a [LoadedPlugin],
}

/// Workloads with a compiled graph, kept across execution ticks.
#[derive(Default)]
pub struct ResidentExecutionSet {
    polled: BTreeMap<String, PolledExecution>,
    frame_driven: BTreeMap<String, FrameDrivenExecution>,
}

impl ResidentExecutionSet {
    /// Bring the resident set in line with `workloads` and report every workload's session and
    /// the artifacts to publish now. Removed or changed workloads are dropped (stopping their
    /// frame drivers).
    pub fn tick_workloads(&mut self, plugins: &ExecutionPlugins<'_>, workloads: &[ExecutionWorkload], state_snapshot: Option<&StateSnapshot>, observed_at_ms: u64) -> ExecutionSnapshot {
        let resources = state_snapshot.map(resources_for_execution).unwrap_or_default();
        let workload_by_id = workloads.iter().map(|workload| (workload.workload_id.as_str(), workload)).collect::<BTreeMap<_, _>>();
        let unchanged = |workload_id: &String, resident: &ExecutionWorkload| workload_by_id.get(workload_id.as_str()).is_some_and(|workload| resident == *workload);
        self.polled.retain(|workload_id, resident| unchanged(workload_id, &resident.workload));
        self.frame_driven.retain(|workload_id, resident| unchanged(workload_id, &resident.workload));

        let mut snapshot = ExecutionSnapshot { sessions: Vec::with_capacity(workloads.len()), artifacts: Vec::new() };
        for workload in workloads {
            match self.tick_workload(plugins, workload, &resources, observed_at_ms) {
                Ok((session, artifacts)) => {
                    snapshot.sessions.push(session);
                    snapshot.artifacts.extend(artifacts);
                }
                Err(error) => snapshot.sessions.push(failed_session(workload, observed_at_ms, error)),
            }
        }
        snapshot
    }

    fn tick_workload(
        &mut self,
        plugins: &ExecutionPlugins<'_>,
        workload: &ExecutionWorkload,
        resources: &BTreeMap<ResourceId, ResourceRecord>,
        observed_at_ms: u64,
    ) -> Result<(ExecutionSessionState, Vec<ExecutionArtifactRecord>), ExecutionError> {
        let id = workload.workload_id.as_str();
        let resolved = resolve_bindings(workload, resources)?;
        if resolved.frames.is_empty() {
            self.frame_driven.remove(id);
            if !self.polled.contains_key(id) {
                let graph = compile_workload_graph(plugins.registry, plugins.loaded_plugins, workload)?;
                self.polled.insert(id.to_string(), PolledExecution::new(workload.clone(), graph));
            }
            let resident = self.polled.get_mut(id).expect("polled execution was just ensured");
            return resident.tick(resolved.resources, observed_at_ms);
        }

        self.polled.remove(id);
        // A camera that moved to another socket (or changed request) restarts the driver.
        if self.frame_driven.get(id).is_some_and(|resident| resident.sources != resolved.frames) {
            self.frame_driven.remove(id);
        }
        if !self.frame_driven.contains_key(id) {
            let graph = compile_workload_graph(plugins.registry, plugins.loaded_plugins, workload)?;
            let resident = FrameDrivenExecution::start(workload.clone(), graph, resolved.frames)?;
            self.frame_driven.insert(id.to_string(), resident);
        }
        let resident = self.frame_driven.get_mut(id).expect("frame driver was just ensured");
        resident.update_context(resolved.resources);
        Ok(resident.snapshot())
    }
}

fn resources_for_execution(snapshot: &StateSnapshot) -> BTreeMap<ResourceId, ResourceRecord> {
    let mut resources = snapshot.state.desired.resources.clone();
    resources.extend(snapshot.state.observed.resources.clone());
    resources
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
