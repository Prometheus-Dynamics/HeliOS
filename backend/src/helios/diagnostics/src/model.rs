use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub generated_at: DateTime<Utc>,
    pub status: HealthStatus,
    pub writable_store: MountReport,
    pub journal: JournalReport,
    pub identity: IdentityReport,
    pub camera: CameraReport,
    pub overlay: OverlayReport,
    pub runtime: RuntimeReport,
    pub managed_bins: ManagedBinsReport,
    pub executable_files: Vec<ExecutableFileReport>,
    pub dynamic_links: Vec<DynamicLinkReport>,
    pub plugins: PluginReport,
    pub orion: OrionReport,
    pub ota: OtaReport,
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
    pub persistent_dir: String,
    pub on_writable_store: bool,
    pub has_files: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct IdentityReport {
    pub machine_id_persisted: bool,
    pub ssh_host_keys_persisted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CameraReport {
    pub startup_preset_declares_camera: bool,
    pub discovered_camera_resources: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct OverlayReport {
    pub path: String,
    pub slot_dirs_present: bool,
    pub image_owned_override_count: usize,
    pub image_owned_overrides: Vec<String>,
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
pub struct ManagedBinsReport {
    pub path: String,
    pub exists: bool,
    pub entries: Vec<ManagedBinEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedBinEntry {
    pub name: String,
    pub exists: bool,
    pub executable: bool,
    pub target: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
pub struct OtaReport {
    pub local_ota_dir: String,
    pub local_ota_dir_exists: bool,
    pub active_slot: Option<String>,
    pub reserve_slot: Option<String>,
    pub updater_socket: String,
    pub updater_socket_exists: bool,
    pub boot_mount: MountReport,
    pub boot_device: Option<String>,
    pub boot_device_exists: bool,
    pub confirm_service_installed: bool,
    pub os_image_ready: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceReport {
    pub unit: String,
    pub active_state: String,
    pub sub_state: String,
    pub main_pid: Option<u32>,
    pub exec_start: Vec<String>,
    pub uses_managed_bin: bool,
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
