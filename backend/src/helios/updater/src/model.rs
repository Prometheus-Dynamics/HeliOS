use std::path::PathBuf;

use orion::{orion_resource_type, orion_runtime_type};

#[orion_runtime_type("helios.system.update.v1")]
pub struct SystemUpdateRuntime;

#[orion_resource_type("system.update.runtime")]
pub struct SystemUpdateRuntimeResource;

#[orion_resource_type("system.update.execution")]
pub struct SystemUpdateExecutionResource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateArtifactClass {
    OsImage,
    PayloadUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationSource {
    OrionWorkload,
    LocalCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateSlot {
    A,
    B,
}

impl UpdateSlot {
    pub fn inactive_for(active: Self) -> Self {
        match active {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

impl UpdateArtifactClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OsImage => "os-image",
            Self::PayloadUpdate => "payload-update",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotLayout {
    pub scheme: String,
    pub active: UpdateSlot,
    pub active_name: String,
    pub active_label: Option<String>,
    pub active_device: Option<PathBuf>,
    pub inactive: Option<UpdateSlot>,
    pub inactive_name: Option<String>,
    pub inactive_label: Option<String>,
    pub inactive_device: Option<PathBuf>,
    pub inactive_exists: bool,
    pub inactive_size_bytes: Option<u64>,
    pub required_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotAction {
    None,
    CreateInactive,
    ResizeInactive { required_size_bytes: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookPhase {
    Preinstall,
    Preswitch,
    Postboot,
    RollbackCleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPlan {
    pub phase: HookPhase,
    pub command: Vec<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateIntent {
    pub artifact_id: String,
    pub artifact_class: UpdateArtifactClass,
    pub version: String,
    pub invocation: InvocationSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateWorkload {
    pub workload_id: String,
    pub artifact_id: String,
    pub assigned_node_id: String,
    pub version: String,
    pub artifact_class: UpdateArtifactClass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignedUpdateExecution {
    pub workload: UpdateWorkload,
    pub phase: UpdatePhase,
    pub status_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateExecutionPlan {
    pub intent: UpdateIntent,
    pub slot_action: SlotAction,
    pub target_slot: Option<UpdateSlot>,
    pub hooks: Vec<HookPlan>,
    pub rollback_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePhase {
    Idle,
    Preflight,
    Downloading,
    Staging,
    SwitchingBoot,
    AwaitingBootSuccess,
    Finalizing,
    RollingBack,
    Completed,
    Failed,
}

impl UpdatePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Preflight => "preflight",
            Self::Downloading => "downloading",
            Self::Staging => "staging",
            Self::SwitchingBoot => "switching_boot",
            Self::AwaitingBootSuccess => "awaiting_boot_success",
            Self::Finalizing => "finalizing",
            Self::RollingBack => "rolling_back",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRunState {
    pub phase: UpdatePhase,
    pub artifact_id: Option<String>,
    pub version: Option<String>,
    pub status_message: String,
}

impl UpdateRunState {
    pub fn idle() -> Self {
        Self { phase: UpdatePhase::Idle, artifact_id: None, version: None, status_message: "idle".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadTarget {
    pub name: String,
    pub revision: String,
    pub binary_path: PathBuf,
}
