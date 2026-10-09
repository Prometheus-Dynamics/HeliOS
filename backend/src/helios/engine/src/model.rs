use std::{collections::BTreeMap, path::PathBuf};

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
    pub daedalus_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphRef {
    ArtifactId(String),
    ResourceId(String),
    InlineSpec(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExecutionBinding {
    pub input: String,
    pub resource_id: String,
    pub node_id: String,
    /// What to ask a camera frame source for; ignored for other resources.
    pub frame_request: FrameRequestOptions,
    /// Camera context (`binding.<input>.context.<field>`): values that go with this camera's
    /// frames, its calibration (`camera.*`) and its mount on the robot (`mount.*`), pushed into
    /// the graph's held structured host inputs (`camera`, `extrinsics`; see
    /// `execution::camera_context`). Frame bindings only.
    pub context: BTreeMap<String, ContextValue>,
}

/// A camera context value: a number (`TypedConfigValue::F64`, or an int) or a name (a string,
/// such as the lens model).
#[derive(Debug, Clone, PartialEq)]
pub enum ContextValue {
    Number(f64),
    Name(String),
}

// Numbers are checked to be finite when decoded, so equality is reflexive.
impl Eq for ContextValue {}

impl ContextValue {
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Name(_) => None,
        }
    }

    pub fn as_name(&self) -> Option<&str> {
        match self {
            Self::Number(_) => None,
            Self::Name(name) => Some(name),
        }
    }
}

/// Optional per-binding frame requirements. The default asks for 8-bit luma at the
/// camera's native size from the service's first camera.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrameRequestOptions {
    /// Camera name (or part of it, or an identity key) when the service serves several.
    pub camera: Option<String>,
    /// Frame size the graph works at; the camera service scales to it.
    pub output_resolution: Option<(u32, u32)>,
    /// Half-size pyramid levels to attach to each frame (the first from the ISP where it can).
    pub pyramid_levels: Option<u8>,
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

impl ExecutionWorkload {
    /// The workload as its compiled graph depends on it: everything but the camera context,
    /// which is pushed into the running graph instead.
    pub fn compiled_shape(&self) -> Self {
        let mut shape = self.clone();
        for binding in &mut shape.bindings {
            binding.context.clear();
        }
        shape
    }
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
