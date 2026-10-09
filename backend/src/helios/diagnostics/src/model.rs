use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub generated_at: DateTime<Utc>,
    pub status: HealthStatus,
    /// The root filesystem: a read-only EROFS slot (p5 or p6).
    pub root: MountReport,
    /// The data partition (p7), kept across updates; tmpfs when it could not be mounted.
    pub data: MountReport,
    /// HeliOS state (`/var/lib/helios`), bind-mounted from the data partition.
    pub writable_store: MountReport,
    pub journal: JournalReport,
    pub identity: IdentityReport,
    pub camera: CameraReport,
    pub runtime: RuntimeReport,
    pub executable_files: Vec<ExecutableFileReport>,
    pub dynamic_links: Vec<DynamicLinkReport>,
    pub plugins: PluginReport,
    pub orion: OrionReport,
    pub update: UpdateReport,
    pub services: Vec<ServiceReport>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Ok,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct MountReport {
    pub path: String,
    pub mounted: bool,
    pub fs_type: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JournalReport {
    pub path: String,
    pub mounted: bool,
    pub source: Option<String>,
    /// Bind-mounted from the data partition.
    pub on_writable_store: bool,
    pub has_files: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityReport {
    /// `/etc/machine-id` is the one kept on the data partition.
    pub machine_id_persisted: bool,
    /// sshd's host keys exist on the data partition.
    pub ssh_host_keys_persisted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CameraReport {
    pub startup_preset_declares_camera: bool,
    pub discovered_camera_resources: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeReport {
    pub loadavg: Option<LoadAverageReport>,
    pub memory: Option<MemoryReport>,
    pub processes: Vec<ProcessRuntimeReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadAverageReport {
    pub one: f32,
    pub five: f32,
    pub fifteen: f32,
    pub running_tasks: u32,
    pub total_tasks: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryReport {
    pub total_kib: u64,
    pub available_kib: Option<u64>,
    pub buffers_kib: Option<u64>,
    pub cached_kib: Option<u64>,
    pub slab_kib: Option<u64>,
    pub reclaimable_slab_kib: Option<u64>,
    pub shmem_kib: Option<u64>,
    pub swap_total_kib: Option<u64>,
    pub swap_free_kib: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcessRuntimeReport {
    pub unit: String,
    pub process_count: usize,
    pub extra_pids: Vec<u32>,
    pub pid: u32,
    pub rss_kib: Option<u64>,
    pub pss_kib: Option<u64>,
    pub private_clean_kib: Option<u64>,
    pub private_dirty_kib: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutableFileReport {
    pub path: String,
    pub exists: bool,
    pub executable: bool,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DynamicLinkReport {
    pub binary: String,
    pub ok: bool,
    pub missing_libraries: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginReport {
    pub path: String,
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrionReport {
    pub run_dir: String,
    pub control_socket: bool,
    pub control_stream_socket: bool,
}

/// The device package's A/B updater (`/usr/lib/board/update`).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateReport {
    pub tool_installed: bool,
    pub confirm_service_loaded: bool,
    pub health_check_installed: bool,
    /// From `/run/board/update.json`.
    pub state: Option<String>,
    pub slot_active: Option<String>,
    pub slot_staged: Option<String>,
    pub version_active: Option<String>,
    pub error: Option<String>,
    pub ab_layout: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceReport {
    pub unit: String,
    pub active_state: String,
    pub sub_state: String,
    pub main_pid: Option<u32>,
    pub exec_start: Vec<String>,
    pub recent_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotSummary {
    pub bundle_dir: String,
    pub archive: Option<String>,
    pub trigger: String,
    pub unit: Option<String>,
    pub generated_at: DateTime<Utc>,
    pub commands: Vec<crate::cmd::CmdResult>,
}
