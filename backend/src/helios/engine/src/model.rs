use std::path::PathBuf;

use orion::{control_plane::WorkloadObservedState, orion_resource_type, orion_runtime_type};

#[orion_runtime_type("helios.engine.execution.v1")]
pub struct EngineExecutionRuntime;

#[orion_resource_type("execution.runtime")]
pub struct EngineRuntimeResource;

#[orion_resource_type("execution.session")]
pub struct ExecutionSessionResource;

#[orion_resource_type("execution.artifact")]
pub struct ExecutionArtifactResource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedPlugin {
    pub path: PathBuf,
    pub plugin_name: Option<String>,
    pub plugin_version: Option<String>,
    pub abi_version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphRef {
    ArtifactId(String),
    ResourceId(String),
    InlineSpec(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionBinding {
    pub input: String,
    pub resource_id: String,
    pub node_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRequirement {
    pub plugin_name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionWorkload {
    pub workload_id: String,
    pub artifact_id: String,
    pub assigned_node_id: String,
    pub graph_ref: GraphRef,
    pub bindings: Vec<ExecutionBinding>,
    pub plugin_requirements: Vec<PluginRequirement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionSessionStatus {
    Pending,
    Assigned,
    Starting,
    Running,
    Stopping,
    Stopped,
    Succeeded,
    Failed,
}

impl ExecutionSessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Assigned => "assigned",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    pub fn to_workload_observed_state(self) -> WorkloadObservedState {
        match self {
            Self::Pending => WorkloadObservedState::Pending,
            Self::Assigned => WorkloadObservedState::Assigned,
            Self::Starting => WorkloadObservedState::Starting,
            Self::Running => WorkloadObservedState::Running,
            Self::Stopping | Self::Stopped => WorkloadObservedState::Stopped,
            Self::Succeeded => WorkloadObservedState::Completed,
            Self::Failed => WorkloadObservedState::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionSessionState {
    pub workload_id: String,
    pub session_id: String,
    pub status: ExecutionSessionStatus,
    pub observed_at_ms: u64,
    pub graph_ref: GraphRef,
    pub bindings: Vec<ExecutionBinding>,
    pub plugin_requirements: Vec<PluginRequirement>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionArtifactRecord {
    pub workload_id: String,
    pub session_id: String,
    pub artifact_id: String,
    pub kind: String,
    pub observed_at_ms: u64,
    pub message: Option<String>,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSnapshot {
    pub node_id: String,
    pub engine_socket_path: PathBuf,
    pub plugin_dirs: Vec<PathBuf>,
    pub loaded_plugins: Vec<LoadedPlugin>,
    pub assigned_workloads: Vec<ExecutionWorkload>,
    pub sessions: Vec<ExecutionSessionState>,
    pub artifacts: Vec<ExecutionArtifactRecord>,
}
