//! Resident, input-driven execution of assigned workloads.
//!
//! Each workload's `GraphDocument` is compiled once and kept until the workload changes. Its
//! graph runs on a driver thread of its own and ticks only when input arrives ([`driver`]): a
//! camera frame for workloads bound to a Styx camera service (a resource with a
//! `styx-frames+unix://` endpoint), a change of the bound resources otherwise. The engine
//! publishes the latest outputs, stats, plan and metrics of every workload as Orion artifacts.

mod bindings;
mod driver;
mod frame_source;
mod graph;
mod outputs;
mod stats;
#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, sync::Arc};

use daedalus::runtime::plugins::PluginRegistry;
use orion::{
    ResourceId,
    control_plane::{ResourceRecord, StateSnapshot},
};
use tokio::sync::Notify;

pub use bindings::STYX_FRAMES_ENDPOINT_SCHEME;
pub use graph::GraphSettings;

use crate::model::{ExecutionArtifactRecord, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload, LoadedPlugin};
use bindings::resolve_bindings;
use driver::WorkloadDriver;
use frame_source::{FrameSource, StyxFrameSource};
use graph::compile_workload_graph;
use outputs::session_id_for;

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

/// Workloads with a compiled graph and a running driver, kept across Orion updates.
pub struct ResidentExecutionSet {
    drivers: BTreeMap<String, WorkloadDriver>,
    settings: GraphSettings,
    outputs_ready: Arc<Notify>,
}

impl Default for ResidentExecutionSet {
    fn default() -> Self {
        Self::new(GraphSettings::default())
    }
}

impl ResidentExecutionSet {
    pub fn new(settings: GraphSettings) -> Self {
        Self { drivers: BTreeMap::new(), settings, outputs_ready: Arc::new(Notify::new()) }
    }

    /// Signalled whenever a workload graph produced outputs (or its driver's state changed), so
    /// the engine can publish a new snapshot.
    pub fn outputs_ready(&self) -> Arc<Notify> {
        self.outputs_ready.clone()
    }

    /// Bring the resident set in line with `workloads`, hand the bound resources to their graphs
    /// and report every workload's session and the artifacts to publish now. Removed or changed
    /// workloads are dropped (stopping their drivers).
    pub fn sync_workloads(&mut self, plugins: &ExecutionPlugins<'_>, workloads: &[ExecutionWorkload], state_snapshot: Option<&StateSnapshot>, observed_at_ms: u64) -> ExecutionSnapshot {
        let resources = state_snapshot.map(resources_for_execution).unwrap_or_default();
        let workload_by_id = workloads.iter().map(|workload| (workload.workload_id.as_str(), workload)).collect::<BTreeMap<_, _>>();
        self.drivers.retain(|workload_id, resident| workload_by_id.get(workload_id.as_str()).is_some_and(|workload| resident.workload == **workload));

        let mut snapshot = ExecutionSnapshot { sessions: Vec::with_capacity(workloads.len()), artifacts: Vec::new() };
        for workload in workloads {
            match self.sync_workload(plugins, workload, &resources) {
                Ok((session, artifacts)) => {
                    snapshot.sessions.push(session);
                    snapshot.artifacts.extend(artifacts);
                }
                Err(error) => {
                    self.drivers.remove(&workload.workload_id);
                    snapshot.sessions.push(failed_session(workload, observed_at_ms, error));
                }
            }
        }
        snapshot
    }

    fn sync_workload(
        &mut self,
        plugins: &ExecutionPlugins<'_>,
        workload: &ExecutionWorkload,
        resources: &BTreeMap<ResourceId, ResourceRecord>,
    ) -> Result<(ExecutionSessionState, Vec<ExecutionArtifactRecord>), ExecutionError> {
        let id = workload.workload_id.as_str();
        let resolved = resolve_bindings(workload, resources)?;
        // A camera that moved to another socket (or a changed request) restarts the driver.
        if self.drivers.get(id).is_some_and(|resident| resident.sources != resolved.frames) {
            self.drivers.remove(id);
        }
        if !self.drivers.contains_key(id) {
            let graph = compile_workload_graph(plugins.registry, plugins.loaded_plugins, workload, self.settings)?;
            let frames = resolved.frames.iter().map(|spec| (spec.clone(), Box::new(StyxFrameSource::new(spec.clone())) as Box<dyn FrameSource>)).collect();
            let driver = WorkloadDriver::start(workload.clone(), graph, frames, self.outputs_ready.clone())?;
            self.drivers.insert(id.to_string(), driver);
        }
        let driver = self.drivers.get_mut(id).expect("driver was just ensured");
        driver.update_context(resolved.resources);
        Ok(driver.snapshot())
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
