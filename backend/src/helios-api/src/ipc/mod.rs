pub mod engine;
pub mod peripherals;
pub mod updater;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, sync::Arc};

use serde::Serialize;
use tokio::sync::{Mutex, RwLock, broadcast};
use tracing::{error, info, warn};
use uuid::Uuid;

use self::engine::EngineConnection;
use self::peripherals::SensorsConnection;
use self::updater::UpdaterConnection;

/// Default IPC socket locations for each runtime.
pub const PERIPHERALS_SOCKET: &str = "/run/helios/peripherals.sock";
pub const UPDATER_SOCKET: &str = "/run/helios/updater.sock";

pub const DEFAULT_JOURNAL_DIR: &str = "/var/lib/helios/journal/ipc";
pub const IPC_JOURNAL_DIR_ENV: &str = "HELIOS_IPC_JOURNAL_DIR";

/// Live handles into each service connection.
pub struct IpcHandles {
    pub engine: EngineConnection,
    sensors: RwLock<Option<Arc<SensorsConnection>>>,
    sensors_connect: Mutex<()>,
    pub updater: Mutex<Option<Arc<UpdaterConnection>>>,
    pub updates: RealtimeUpdateBus,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeUpdateOrigin {
    Http,
    Ws,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeUpdateDomain {
    Api,
    Device,
    Localization,
    Media,
    Pipelines,
    Settings,
    Streams,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum RealtimeUpdateKind {
    #[serde(rename = "api")]
    Api,
    #[serde(rename = "device.hardware")]
    DeviceHardware,
    #[serde(rename = "device.imu")]
    DeviceImu,
    #[serde(rename = "device.settings")]
    DeviceSettings,
    #[serde(rename = "localization.config")]
    LocalizationConfig,
    #[serde(rename = "localization.maps")]
    LocalizationMaps,
    #[serde(rename = "localization.profiles")]
    LocalizationProfiles,
    #[serde(rename = "localization.sources")]
    LocalizationSources,
    #[serde(rename = "media.assets")]
    MediaAssets,
    #[serde(rename = "media.imu")]
    MediaImu,
    #[serde(rename = "media.labels")]
    MediaLabels,
    #[serde(rename = "media.metadata")]
    MediaMetadata,
    #[serde(rename = "pipelines.graphs")]
    PipelinesGraphs,
    #[serde(rename = "settings.device")]
    SettingsDevice,
    #[serde(rename = "settings.plugins")]
    SettingsPlugins,
    #[serde(rename = "settings.updater")]
    SettingsUpdater,
    #[serde(rename = "streams.controls")]
    StreamsControls,
    #[serde(rename = "streams.lifecycle")]
    StreamsLifecycle,
    #[serde(rename = "streams.pipeline")]
    StreamsPipeline,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeUpdateOperation {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeUpdateEntity {
    Api,
    DeviceImu,
    DeviceHardware,
    DeviceSettings,
    LocalizationConfig,
    LocalizationMap,
    LocalizationProfile,
    LocalizationSource,
    MediaAsset,
    MediaImu,
    MediaLabel,
    MediaMetadata,
    PipelineGraph,
    Plugin,
    Stream,
    StreamControl,
    StreamPipeline,
    Updater,
}

impl RealtimeUpdateKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::DeviceHardware => "device.hardware",
            Self::DeviceImu => "device.imu",
            Self::DeviceSettings => "device.settings",
            Self::LocalizationConfig => "localization.config",
            Self::LocalizationMaps => "localization.maps",
            Self::LocalizationProfiles => "localization.profiles",
            Self::LocalizationSources => "localization.sources",
            Self::MediaAssets => "media.assets",
            Self::MediaImu => "media.imu",
            Self::MediaLabels => "media.labels",
            Self::MediaMetadata => "media.metadata",
            Self::PipelinesGraphs => "pipelines.graphs",
            Self::SettingsDevice => "settings.device",
            Self::SettingsPlugins => "settings.plugins",
            Self::SettingsUpdater => "settings.updater",
            Self::StreamsControls => "streams.controls",
            Self::StreamsLifecycle => "streams.lifecycle",
            Self::StreamsPipeline => "streams.pipeline",
        }
    }

    pub const fn domain(self) -> RealtimeUpdateDomain {
        match self {
            Self::Api => RealtimeUpdateDomain::Api,
            Self::DeviceHardware | Self::DeviceImu | Self::DeviceSettings => RealtimeUpdateDomain::Device,
            Self::LocalizationConfig | Self::LocalizationMaps | Self::LocalizationProfiles | Self::LocalizationSources => RealtimeUpdateDomain::Localization,
            Self::MediaAssets | Self::MediaImu | Self::MediaLabels | Self::MediaMetadata => RealtimeUpdateDomain::Media,
            Self::PipelinesGraphs => RealtimeUpdateDomain::Pipelines,
            Self::SettingsDevice | Self::SettingsPlugins | Self::SettingsUpdater => RealtimeUpdateDomain::Settings,
            Self::StreamsControls | Self::StreamsLifecycle | Self::StreamsPipeline => RealtimeUpdateDomain::Streams,
        }
    }
}

impl std::fmt::Display for RealtimeUpdateKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RealtimeUpdateEvent {
    pub seq: u64,
    pub timestamp_ms: u64,
    pub origin: RealtimeUpdateOrigin,
    pub domain: RealtimeUpdateDomain,
    pub kind: RealtimeUpdateKind,
    pub operation: RealtimeUpdateOperation,
    pub entity: RealtimeUpdateEntity,
    pub revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

pub struct RealtimeUpdateBus {
    tx: broadcast::Sender<RealtimeUpdateEvent>,
    seq: AtomicU64,
    revisions: StdMutex<HashMap<(RealtimeUpdateKind, Option<String>), u64>>,
}

impl Default for RealtimeUpdateBus {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx, seq: AtomicU64::new(0), revisions: StdMutex::new(HashMap::new()) }
    }
}

impl RealtimeUpdateBus {
    pub fn subscribe(&self) -> broadcast::Receiver<RealtimeUpdateEvent> {
        self.tx.subscribe()
    }

    pub fn publish(&self, origin: RealtimeUpdateOrigin, kind: RealtimeUpdateKind, path: impl Into<String>, method: Option<String>, request_id: Option<String>) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let path = path.into();
        let resource_id = resource_id_for_event(kind, &path);
        let operation = operation_for_event(origin, method.as_deref());
        let entity = entity_for_event(kind);
        let revision = self.next_revision(kind, resource_id.as_ref());
        let event = RealtimeUpdateEvent { seq, timestamp_ms: now_ms(), origin, domain: kind.domain(), kind, operation, entity, revision, resource_id, path, method, request_id };
        let _ = self.tx.send(event);
    }

    fn next_revision(&self, kind: RealtimeUpdateKind, resource_id: Option<&String>) -> u64 {
        let mut revisions = self.revisions.lock().expect("realtime revisions poisoned");
        let key = (kind, resource_id.cloned());
        let next = revisions.get(&key).copied().unwrap_or(0).saturating_add(1);
        revisions.insert(key, next);
        next
    }
}

fn resource_id_for_event(kind: RealtimeUpdateKind, path: &str) -> Option<String> {
    let trimmed = path.split('?').next().unwrap_or(path);
    let segments: Vec<&str> = trimmed.trim_matches('/').split('/').collect();
    if segments.is_empty() {
        return None;
    }

    match kind {
        RealtimeUpdateKind::StreamsControls | RealtimeUpdateKind::StreamsLifecycle | RealtimeUpdateKind::StreamsPipeline => {
            extract_segment_after_prefix(&segments, &["v1", "streams"]).or_else(|| extract_segment_after_prefix(&segments, &["v1", "ws", "streams"]))
        }
        RealtimeUpdateKind::PipelinesGraphs => extract_segment_after_prefix(&segments, &["v1", "pipelines"]).or_else(|| extract_segment_after_prefix(&segments, &["v1", "ws", "pipelines"])),
        RealtimeUpdateKind::MediaAssets | RealtimeUpdateKind::MediaImu | RealtimeUpdateKind::MediaLabels | RealtimeUpdateKind::MediaMetadata => {
            extract_segment_after_prefix(&segments, &["v1", "media"])
        }
        RealtimeUpdateKind::LocalizationMaps => extract_segment_after_prefix(&segments, &["v1", "localization", "maps"]),
        RealtimeUpdateKind::LocalizationProfiles => {
            extract_segment_after_prefix(&segments, &["v1", "localization", "profiles"]).or_else(|| extract_segment_after_prefix(&segments, &["v1", "localization", "profile"]))
        }
        RealtimeUpdateKind::Api => Some("api".to_string()),
        RealtimeUpdateKind::DeviceHardware => Some("device.hardware".to_string()),
        RealtimeUpdateKind::DeviceImu => Some("device.imu".to_string()),
        RealtimeUpdateKind::DeviceSettings => Some("device.settings".to_string()),
        RealtimeUpdateKind::LocalizationConfig => Some("localization.config".to_string()),
        RealtimeUpdateKind::LocalizationSources => Some("localization.sources".to_string()),
        RealtimeUpdateKind::SettingsDevice => Some("settings.device".to_string()),
        RealtimeUpdateKind::SettingsPlugins => Some("settings.plugins".to_string()),
        RealtimeUpdateKind::SettingsUpdater => Some("settings.updater".to_string()),
    }
}

fn operation_for_event(origin: RealtimeUpdateOrigin, method: Option<&str>) -> RealtimeUpdateOperation {
    match origin {
        RealtimeUpdateOrigin::Ws => RealtimeUpdateOperation::Update,
        RealtimeUpdateOrigin::Http => match method.unwrap_or_default() {
            "post" => RealtimeUpdateOperation::Create,
            "delete" => RealtimeUpdateOperation::Delete,
            _ => RealtimeUpdateOperation::Update,
        },
    }
}

fn entity_for_event(kind: RealtimeUpdateKind) -> RealtimeUpdateEntity {
    match kind {
        RealtimeUpdateKind::Api => RealtimeUpdateEntity::Api,
        RealtimeUpdateKind::DeviceHardware => RealtimeUpdateEntity::DeviceHardware,
        RealtimeUpdateKind::DeviceImu => RealtimeUpdateEntity::DeviceImu,
        RealtimeUpdateKind::DeviceSettings | RealtimeUpdateKind::SettingsDevice => RealtimeUpdateEntity::DeviceSettings,
        RealtimeUpdateKind::LocalizationConfig => RealtimeUpdateEntity::LocalizationConfig,
        RealtimeUpdateKind::LocalizationMaps => RealtimeUpdateEntity::LocalizationMap,
        RealtimeUpdateKind::LocalizationProfiles => RealtimeUpdateEntity::LocalizationProfile,
        RealtimeUpdateKind::LocalizationSources => RealtimeUpdateEntity::LocalizationSource,
        RealtimeUpdateKind::MediaAssets => RealtimeUpdateEntity::MediaAsset,
        RealtimeUpdateKind::MediaImu => RealtimeUpdateEntity::MediaImu,
        RealtimeUpdateKind::MediaLabels => RealtimeUpdateEntity::MediaLabel,
        RealtimeUpdateKind::MediaMetadata => RealtimeUpdateEntity::MediaMetadata,
        RealtimeUpdateKind::PipelinesGraphs => RealtimeUpdateEntity::PipelineGraph,
        RealtimeUpdateKind::SettingsPlugins => RealtimeUpdateEntity::Plugin,
        RealtimeUpdateKind::SettingsUpdater => RealtimeUpdateEntity::Updater,
        RealtimeUpdateKind::StreamsControls => RealtimeUpdateEntity::StreamControl,
        RealtimeUpdateKind::StreamsLifecycle => RealtimeUpdateEntity::Stream,
        RealtimeUpdateKind::StreamsPipeline => RealtimeUpdateEntity::StreamPipeline,
    }
}

fn extract_segment_after_prefix(segments: &[&str], prefix: &[&str]) -> Option<String> {
    if segments.len() <= prefix.len() || segments.get(..prefix.len()) != Some(prefix) {
        return None;
    }
    let value = segments[prefix.len()].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u64::MAX as u128) as u64
}

fn is_transient_peripherals_connect_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("no such file or directory") || msg.contains("connection refused") || msg.contains("connection reset") || msg.contains("timed out") || msg.contains("handshake io error")
}

fn log_peripherals_connect_failure(err: &str, phase: &'static str) {
    if is_transient_peripherals_connect_error(err) {
        warn!(error = %err, phase, "peripherals IPC unavailable (transient); will retry");
    } else {
        error!(error = %err, phase, "failed to connect to peripherals IPC");
    }
}

/// Entry point for wiring all IPC endpoints needed by the control plane.
pub async fn connect_all() -> IpcHandles {
    let engine = engine::connect_engine_best_effort().await;

    let sensors = match peripherals::connect_sensors().await {
        Ok(conn) => {
            info!("connected to peripherals IPC");
            Some(conn)
        }
        Err(err) => {
            let detail = err.to_string();
            log_peripherals_connect_failure(&detail, "startup");
            None
        }
    };

    let updater = match updater::connect_updater().await {
        Ok(conn) => {
            info!("connected to updater IPC");
            Some(Arc::new(conn))
        }
        Err(err) => {
            error!(%err, "failed to connect to updater IPC");
            None
        }
    };

    IpcHandles { engine, sensors: RwLock::new(sensors.map(Arc::new)), sensors_connect: Mutex::new(()), updater: Mutex::new(updater), updates: RealtimeUpdateBus::default() }
}

impl IpcHandles {
    /// Ensure a live peripherals connection, attempting to reconnect on demand.
    pub async fn ensure_sensors(&self) -> Option<Arc<SensorsConnection>> {
        if let Some(conn) = self.sensors.read().await.as_ref().cloned() {
            return Some(conn);
        }

        let _connect_guard = self.sensors_connect.lock().await;
        if let Some(conn) = self.sensors.read().await.as_ref().cloned() {
            return Some(conn);
        }
        match peripherals::connect_sensors().await {
            Ok(conn) => {
                let conn = Arc::new(conn);
                let mut guard = self.sensors.write().await;
                *guard = Some(conn.clone());
                info!("connected to peripherals IPC");
                Some(conn)
            }
            Err(err) => {
                let detail = err.to_string();
                log_peripherals_connect_failure(&detail, "on_demand");
                None
            }
        }
    }

    pub async fn invalidate_sensors(&self) {
        let mut guard = self.sensors.write().await;
        *guard = None;
    }

    pub fn subscribe_realtime_updates(&self) -> broadcast::Receiver<RealtimeUpdateEvent> {
        self.updates.subscribe()
    }

    pub fn publish_realtime_update(&self, origin: RealtimeUpdateOrigin, kind: RealtimeUpdateKind, path: impl Into<String>, method: Option<String>, request_id: Option<String>) {
        self.updates.publish(origin, kind, path, method, request_id);
    }
}

/// Ensure journal directory exists.
pub fn ensure_journal_dir() -> std::io::Result<()> {
    fs::create_dir_all(journal_dir())
}

pub fn journal_dir() -> PathBuf {
    std::env::var_os(IPC_JOURNAL_DIR_ENV).filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_JOURNAL_DIR))
}

pub fn journal_path(file_name: &str) -> PathBuf {
    journal_dir().join(file_name)
}

pub fn command_id_from_context(label: &str) -> lib_ipc::types::CommandId {
    let context = crate::http::error::current_error_context();
    if let Some(request_id) = context.request_id.as_deref()
        && let Ok(uuid) = Uuid::parse_str(request_id)
    {
        let seq = context.next_sequence();
        let name = format!("{label}:{seq}");
        let derived = Uuid::new_v5(&uuid, name.as_bytes());
        return lib_ipc::types::CommandId::from_uuid(derived);
    }
    lib_ipc::types::CommandId::new()
}
