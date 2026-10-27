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
    /// Set when the input is another pipeline's output (`source: "pipeline"`) instead of a
    /// resource; `resource_id` is then empty. See `execution::pipeline_inputs`.
    pub pipeline: Option<PipelineInputSource>,
}

/// Default for [`PipelineInputSource::max_wait_ms`].
pub const DEFAULT_PIPELINE_INPUT_MAX_WAIT_MS: u32 = 50;

/// A graph input fed with the latest output of another pipeline (on the in-process pipeline bus).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineInputSource {
    /// The producing pipeline's id (its workload id without the `pipeline.` prefix).
    pub pipeline: String,
    /// The producer's host output port.
    pub port: String,
    /// How long a frame waits for the producer to finish the same frame before the graph ticks
    /// with the producer's latest value anyway (`0` never waits).
    pub max_wait_ms: u32,
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
    /// The exact frame rate the camera runs at (the camera's stored fps, set through the camera
    /// controls). Unset asks for the camera's default rate.
    pub fps: Option<u32>,
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
    /// The CPU core the graph thread is pinned to (`cpu` in the config); unset leaves it to the
    /// scheduler. A change restarts the graph (it is part of the compiled shape).
    pub cpu: Option<u32>,
    /// The nice value of the graph thread (`nice` in the config, 0 to 19: a lower priority); unset
    /// keeps the default. A change restarts the graph.
    pub nice: Option<u32>,
    /// The saved map the graph starts from, fed to its `map` input when it starts (`map.id` and
    /// `map.path` in the config). A change restarts the graph: a map is the start of a run.
    pub map: Option<MapSource>,
    /// Save requests so far (`map.save`). Each new value asks the running graph to save its map
    /// once (see `execution::map_io`); it does not restart the graph, so it is not compiled.
    pub map_save: u64,
}

/// A saved map a graph starts from (`docs/map-format.md` in Eidos): the map's id and the file on
/// the board that holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapSource {
    pub id: String,
    pub path: PathBuf,
}

impl ExecutionWorkload {
    /// The workload as its compiled graph depends on it: the camera context's values are pushed
    /// into the running graph instead, so only which groups it has (`camera`, `mount`) count. A
    /// held input cannot be emptied again, so a group that goes away (a mount removed)
    /// recompiles the graph.
    pub fn compiled_shape(&self) -> Self {
        let mut shape = self.clone();
        for binding in &mut shape.bindings {
            binding.context = binding.context.keys().map(|key| (key.split('.').next().unwrap_or_default().to_string(), ContextValue::Name(String::new()))).collect();
        }
        shape.map_save = 0;
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
    pub catalog_path: Option<PathBuf>,
    /// The NetworkTables connection, once the publisher runs.
    pub nt4: Option<crate::nt4::Nt4Status>,
    pub plugin_dirs: Vec<PathBuf>,
    pub loaded_plugins: Vec<LoadedPlugin>,
    pub assigned_workloads: Vec<ExecutionWorkload>,
    pub sessions: Vec<ExecutionSessionState>,
    pub artifacts: Vec<ExecutionArtifactRecord>,
}
