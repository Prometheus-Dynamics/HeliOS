use std::time::Duration;

use crate::artifact::ReleaseManifest;
use lib_ipc::types::{CommandId, RequestIdentity, Timestamp};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<tracing::Level> for LogLevel {
    fn from(level: tracing::Level) -> Self {
        match level {
            tracing::Level::TRACE => Self::Trace,
            tracing::Level::DEBUG => Self::Debug,
            tracing::Level::INFO => Self::Info,
            tracing::Level::WARN => Self::Warn,
            tracing::Level::ERROR => Self::Error,
        }
    }
}

impl From<LogLevel> for tracing::Level {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Trace => tracing::Level::TRACE,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Error => tracing::Level::ERROR,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum UpdaterCommand {
    StageRelease {
        command_id: CommandId,
        update_id: Uuid,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        manifest: ReleaseManifest,
    },
    Cancel {
        command_id: CommandId,
        update_id: Uuid,
    },
    ApplyRelease {
        command_id: CommandId,
        update_id: Uuid,
        window: MaintenanceWindow,
    },
    Rollback {
        command_id: CommandId,
        update_id: Uuid,
    },
    QueryState {
        command_id: CommandId,
    },
    QueryStorage {
        command_id: CommandId,
    },
    PreflightRelease {
        command_id: CommandId,
        update_id: Uuid,
    },
}

impl RequestIdentity for UpdaterCommand {
    fn request_id(&self) -> CommandId {
        match self {
            Self::StageRelease { command_id, .. }
            | Self::Cancel { command_id, .. }
            | Self::ApplyRelease { command_id, .. }
            | Self::Rollback { command_id, .. }
            | Self::QueryState { command_id }
            | Self::QueryStorage { command_id }
            | Self::PreflightRelease { command_id, .. } => *command_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct MaintenanceWindow {
    #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
    pub start: Timestamp,
    #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
    pub duration: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct UrlArtifact {
    #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
    pub url: Url,
    pub size_bytes: Option<u64>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct StorageDirectoryReport {
    pub path: String,
    pub usage_bytes: u64,
    pub available_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct UpdaterStorageReport {
    pub cache: StorageDirectoryReport,
    pub work: StorageDirectoryReport,
    pub service_releases: StorageDirectoryReport,
    pub frontend_releases: StorageDirectoryReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStage {
    Idle,
    Downloading,
    Verifying,
    AwaitingWindow,
    Applying,
    Rebooting,
    Complete,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct UpdateState {
    pub update_id: Uuid,
    pub stage: UpdateStage,
    pub progress_percent: Option<u8>,
    pub last_error: Option<String>,
    #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
    pub started_at: Option<Timestamp>,
    #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
    pub finished_at: Option<Timestamp>,
    pub artifacts: Vec<UrlArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightVerdict {
    Ready,
    GrowIntoGap,
    OfflineDataBorrow,
    TargetTooSmall,
    NeedsDataResize,
    ClearDataDir,
    ReplaySourceRequired,
    WorkDirFull,
    InvalidArtifact,
    SingleSlotDisabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct PreflightReport {
    pub update_id: Uuid,
    pub ready: bool,
    pub verdict: PreflightVerdict,
    pub summary: String,
    pub artifact_kind: Option<String>,
    pub target_device: Option<String>,
    pub image_size_bytes: Option<u64>,
    pub target_size_bytes: Option<u64>,
    pub gap_after_bytes: Option<u64>,
    pub additional_from_data_bytes: Option<u64>,
    pub data_dir_available_bytes: Option<u64>,
    pub work_dir_available_bytes: Option<u64>,
    pub clear_bytes: Option<u64>,
    pub single_slot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum UpdaterEvent {
    Ack {
        command_id: CommandId,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        processed_at: Timestamp,
    },
    Nack {
        command_id: CommandId,
        reason: String,
        retryable: bool,
    },
    StageProgress {
        update_id: Uuid,
        percent: u8,
        detail: Option<String>,
    },
    StageComplete {
        update_id: Uuid,
    },
    ApplyScheduled {
        update_id: Uuid,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        eta: Timestamp,
    },
    ApplyComplete {
        update_id: Uuid,
        reboot_required: bool,
    },
    RollbackTriggered {
        update_id: Uuid,
        reason: String,
    },
    StateSnapshot {
        active_update: Option<UpdateState>,
        cache_usage_bytes: u64,
    },
    StorageReport {
        report: UpdaterStorageReport,
    },
    PreflightReport {
        report: PreflightReport,
    },
    Heartbeat {
        uptime_ms: u64,
        sequence: u64,
        stage_queue_depth: u32,
    },
    LogRecord {
        level: LogLevel,
        span: Vec<String>,
        message: String,
    },
}

pub mod wire {
    pub use super::{
        LogLevel, MaintenanceWindow, PreflightReport, PreflightVerdict, StorageDirectoryReport, UpdateStage, UpdateState, UpdaterCommand, UpdaterEvent, UpdaterStorageReport, UrlArtifact,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use lib_ipc::wire::{FrameFlags, ServiceKind, StreamKind};
    use serde_json::Value;

    fn assert_ipc_round_trip<T>(stream: StreamKind, value: &T) -> T
    where
        T: lib_ipc::archive::TransportEncode + rkyv::Archive,
        T::Archived: for<'a> rkyv::bytecheck::CheckBytes<lib_ipc::archive::DecodeValidator<'a>> + rkyv::Deserialize<T, lib_ipc::archive::DecodeStrategy>,
    {
        let frame = lib_ipc::frame::Frame::encode_payload(ServiceKind::Updater, stream, CommandId::new(), FrameFlags::empty(), value).expect("encode ipc payload");
        frame.decode_payload().expect("decode ipc payload")
    }

    fn sample_update_state() -> UpdateState {
        UpdateState {
            update_id: Uuid::new_v4(),
            stage: UpdateStage::Applying,
            progress_percent: Some(50),
            last_error: Some("none".into()),
            started_at: Some(Utc::now()),
            finished_at: None,
            artifacts: vec![UrlArtifact { url: Url::parse("https://example.org/update").expect("url"), size_bytes: Some(1024), checksum: Some("abc123".into()) }],
        }
    }

    fn sample_commands() -> Vec<UpdaterCommand> {
        vec![
            UpdaterCommand::StageRelease {
                command_id: CommandId::new(),
                update_id: Uuid::new_v4(),
                manifest: ReleaseManifest { update_id: Some(Uuid::new_v4()), version: Some("v2026.1.0".into()), artifacts: Vec::new(), metadata_json: "{}".into() },
            },
            UpdaterCommand::Cancel { command_id: CommandId::new(), update_id: Uuid::new_v4() },
            UpdaterCommand::ApplyRelease { command_id: CommandId::new(), update_id: Uuid::new_v4(), window: MaintenanceWindow { start: Utc::now(), duration: Duration::from_secs(30) } },
            UpdaterCommand::Rollback { command_id: CommandId::new(), update_id: Uuid::new_v4() },
            UpdaterCommand::QueryState { command_id: CommandId::new() },
            UpdaterCommand::QueryStorage { command_id: CommandId::new() },
            UpdaterCommand::PreflightRelease { command_id: CommandId::new(), update_id: Uuid::new_v4() },
        ]
    }

    fn sample_events() -> Vec<UpdaterEvent> {
        let storage = UpdaterStorageReport {
            cache: StorageDirectoryReport { path: "/var/lib/helios/ota/cache".into(), usage_bytes: 4096, available_bytes: Some(1_048_576) },
            work: StorageDirectoryReport { path: "/var/lib/helios/ota/work".into(), usage_bytes: 2048, available_bytes: Some(1_048_576) },
            service_releases: StorageDirectoryReport { path: "/opt/helios/releases/services".into(), usage_bytes: 8192, available_bytes: Some(1_048_576) },
            frontend_releases: StorageDirectoryReport { path: "/opt/helios/releases/frontend".into(), usage_bytes: 1024, available_bytes: Some(1_048_576) },
        };
        vec![
            UpdaterEvent::Ack { command_id: CommandId::new(), processed_at: Utc::now() },
            UpdaterEvent::Nack { command_id: CommandId::new(), reason: "error".into(), retryable: false },
            UpdaterEvent::StageProgress { update_id: Uuid::new_v4(), percent: 42, detail: Some("downloading".into()) },
            UpdaterEvent::StageComplete { update_id: Uuid::new_v4() },
            UpdaterEvent::ApplyScheduled { update_id: Uuid::new_v4(), eta: Utc::now() },
            UpdaterEvent::ApplyComplete { update_id: Uuid::new_v4(), reboot_required: true },
            UpdaterEvent::RollbackTriggered { update_id: Uuid::new_v4(), reason: "failure".into() },
            UpdaterEvent::StateSnapshot { active_update: Some(sample_update_state()), cache_usage_bytes: 4096 },
            UpdaterEvent::StorageReport { report: storage },
            UpdaterEvent::PreflightReport {
                report: PreflightReport {
                    update_id: Uuid::new_v4(),
                    ready: false,
                    verdict: PreflightVerdict::ClearDataDir,
                    summary: "clear space and retry".into(),
                    artifact_kind: Some("disk-image".into()),
                    target_device: Some("/dev/mmcblk0p3".into()),
                    image_size_bytes: Some(97_140_736),
                    target_size_bytes: Some(96_468_992),
                    gap_after_bytes: Some(4_194_304),
                    additional_from_data_bytes: Some(3_145_728),
                    data_dir_available_bytes: Some(1_048_576),
                    work_dir_available_bytes: Some(8_388_608),
                    clear_bytes: Some(2_097_152),
                    single_slot: false,
                },
            },
            UpdaterEvent::Heartbeat { uptime_ms: 1234, sequence: 2, stage_queue_depth: 1 },
            UpdaterEvent::LogRecord { level: LogLevel::Warn, span: vec!["updater".into()], message: "warn".into() },
        ]
    }

    fn check_round_trip<T>(stream: StreamKind, value: &T) -> Result<(), String>
    where
        T: Serialize + for<'de> Deserialize<'de> + Clone + lib_ipc::archive::TransportEncode + rkyv::Archive,
        T::Archived: for<'a> rkyv::bytecheck::CheckBytes<lib_ipc::archive::DecodeValidator<'a>> + rkyv::Deserialize<T, lib_ipc::archive::DecodeStrategy>,
    {
        let decoded: T = assert_ipc_round_trip(stream, value);

        let original_json: Value = serde_json::to_value(value).map_err(|err| err.to_string())?;
        let decoded_json: Value = serde_json::to_value(&decoded).map_err(|err| err.to_string())?;
        if original_json != decoded_json {
            return Err("serde_json payload mismatch".into());
        }

        Ok(())
    }

    #[test]
    fn updater_event_roundtrip() {
        for event in sample_events() {
            if let Err(err) = check_round_trip(StreamKind::Event, &event) {
                panic!("updater event round-trip failed ({event:?}): {err}");
            }
        }
    }

    #[test]
    fn updater_command_roundtrip() {
        for command in sample_commands() {
            if let Err(err) = check_round_trip(StreamKind::Request, &command) {
                panic!("updater command round-trip failed ({command:?}): {err}");
            }
        }
    }
}
