use std::time::Duration;

use crate::artifact::ReleaseManifest;
use lib_ipc::frame::MessageKind;
use lib_ipc::protocol::ControlEvent;
use lib_ipc::server::ServerEvent;
use lib_ipc::types::{CommandId, Timestamp};
use serde::{Deserialize, Serialize};
use tracing::error;
use url::Url;
use uuid::Uuid;

use bincode::{Decode, Encode};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Encode, Decode)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum UpdaterCommand {
    StageRelease {
        command_id: CommandId,
        #[bincode(with_serde)]
        update_id: Uuid,
        #[bincode(with_serde)]
        manifest: ReleaseManifest,
    },
    Cancel {
        command_id: CommandId,
        #[bincode(with_serde)]
        update_id: Uuid,
    },
    ApplyRelease {
        command_id: CommandId,
        #[bincode(with_serde)]
        update_id: Uuid,
        window: MaintenanceWindow,
    },
    Rollback {
        command_id: CommandId,
        #[bincode(with_serde)]
        update_id: Uuid,
    },
    QueryState {
        command_id: CommandId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct MaintenanceWindow {
    #[bincode(with_serde)]
    pub start: Timestamp,
    pub duration: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode)]
pub struct UrlArtifact {
    #[bincode(with_serde)]
    pub url: Url,
    pub size_bytes: Option<u64>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode)]
pub struct UpdateState {
    #[bincode(with_serde)]
    pub update_id: Uuid,
    pub stage: UpdateStage,
    pub progress_percent: Option<u8>,
    pub last_error: Option<String>,
    #[bincode(with_serde)]
    pub started_at: Option<Timestamp>,
    #[bincode(with_serde)]
    pub finished_at: Option<Timestamp>,
    pub artifacts: Vec<UrlArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum UpdaterEvent {
    Control(ControlEvent),
    StageProgress {
        #[bincode(with_serde)]
        update_id: Uuid,
        percent: u8,
        detail: Option<String>,
    },
    StageComplete {
        #[bincode(with_serde)]
        update_id: Uuid,
    },
    ApplyScheduled {
        #[bincode(with_serde)]
        update_id: Uuid,
        #[bincode(with_serde)]
        eta: Timestamp,
    },
    ApplyComplete {
        #[bincode(with_serde)]
        update_id: Uuid,
        reboot_required: bool,
    },
    RollbackTriggered {
        #[bincode(with_serde)]
        update_id: Uuid,
        reason: String,
    },
    StateSnapshot {
        active_update: Option<UpdateState>,
        cache_usage_bytes: u64,
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
    Unknown {
        kind: u16,
        payload: Vec<u8>,
    },
}

impl From<ControlEvent> for UpdaterEvent {
    fn from(value: ControlEvent) -> Self {
        Self::Control(value)
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdaterCommandKind {
    StageRelease = 0,
    Cancel = 1,
    ApplyRelease = 2,
    Rollback = 3,
    QueryState = 4,
}

impl UpdaterCommandKind {
    const fn to_u16(self) -> u16 {
        self as u16
    }

    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::StageRelease),
            1 => Some(Self::Cancel),
            2 => Some(Self::ApplyRelease),
            3 => Some(Self::Rollback),
            4 => Some(Self::QueryState),
            _ => None,
        }
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdaterEventKind {
    Control = 0,
    StageProgress = 1,
    StageComplete = 2,
    ApplyScheduled = 3,
    ApplyComplete = 4,
    RollbackTriggered = 5,
    StateSnapshot = 6,
    Heartbeat = 7,
    LogRecord = 8,
}

impl UpdaterEventKind {
    const fn to_u16(self) -> u16 {
        self as u16
    }

    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Control),
            1 => Some(Self::StageProgress),
            2 => Some(Self::StageComplete),
            3 => Some(Self::ApplyScheduled),
            4 => Some(Self::ApplyComplete),
            5 => Some(Self::RollbackTriggered),
            6 => Some(Self::StateSnapshot),
            7 => Some(Self::Heartbeat),
            8 => Some(Self::LogRecord),
            _ => None,
        }
    }
}

#[allow(unreachable_code)]
const _: () = {
    lib_ipc::tagged_enum! {
        impl crate::ipc::UpdaterCommand => crate::ipc::UpdaterCommandKind {
            struct StageRelease { command_id: CommandId, update_id: Uuid => with_serde, manifest: ReleaseManifest => with_serde },
            struct Cancel { command_id: CommandId, update_id: Uuid => with_serde },
            struct ApplyRelease { command_id: CommandId, update_id: Uuid => with_serde, window: MaintenanceWindow },
            struct Rollback { command_id: CommandId, update_id: Uuid => with_serde },
            struct QueryState { command_id: CommandId },
        }
    }

    lib_ipc::tagged_enum! {
        impl crate::ipc::UpdaterEvent => crate::ipc::UpdaterEventKind, unknown = Unknown {
            struct StageProgress { update_id: Uuid => with_serde, percent: u8, detail: Option<String> },
            struct StageComplete { update_id: Uuid => with_serde },
            struct ApplyScheduled { update_id: Uuid => with_serde, eta: Timestamp => with_serde },
            struct ApplyComplete { update_id: Uuid => with_serde, reboot_required: bool },
            struct RollbackTriggered { update_id: Uuid => with_serde, reason: String },
            struct StateSnapshot { active_update: Option<UpdateState>, cache_usage_bytes: u64 },
            struct Heartbeat { uptime_ms: u64, sequence: u64, stage_queue_depth: u32 },
            struct LogRecord { level: LogLevel, span: Vec<String>, message: String },
            tuple Control (ControlEvent),
        }
    }
};

impl ServerEvent for UpdaterEvent {
    fn message_kind(&self) -> MessageKind {
        match self {
            Self::Control(_) => MessageKind::Control,
            Self::Heartbeat { .. } => MessageKind::Heartbeat,
            Self::StageProgress { .. }
            | Self::StageComplete { .. }
            | Self::ApplyScheduled { .. }
            | Self::ApplyComplete { .. }
            | Self::RollbackTriggered { .. }
            | Self::StateSnapshot { .. }
            | Self::LogRecord { .. }
            | Self::Unknown { .. } => MessageKind::Event,
        }
    }

    fn as_control(&self) -> Option<&ControlEvent> {
        match self {
            Self::Control(event) => Some(event),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use lib_ipc::envelope::{TaggedDecodeError, TaggedEnvelope};
    use lib_ipc::protocol::{AckEvent, ControlEvent, NackEvent};
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::Value;

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

    fn sample_events() -> Vec<UpdaterEvent> {
        vec![
            UpdaterEvent::Control(ControlEvent::Ack(AckEvent { command_id: CommandId::new(), processed_at: Utc::now() })),
            UpdaterEvent::Control(ControlEvent::Nack(NackEvent { command_id: CommandId::new(), reason: "error".into(), retryable: false })),
            UpdaterEvent::StageProgress { update_id: Uuid::new_v4(), percent: 42, detail: Some("downloading".into()) },
            UpdaterEvent::StageComplete { update_id: Uuid::new_v4() },
            UpdaterEvent::ApplyScheduled { update_id: Uuid::new_v4(), eta: Utc::now() },
            UpdaterEvent::ApplyComplete { update_id: Uuid::new_v4(), reboot_required: true },
            UpdaterEvent::RollbackTriggered { update_id: Uuid::new_v4(), reason: "failure".into() },
            UpdaterEvent::StateSnapshot { active_update: Some(sample_update_state()), cache_usage_bytes: 4096 },
            UpdaterEvent::Heartbeat { uptime_ms: 1234, sequence: 2, stage_queue_depth: 1 },
            UpdaterEvent::LogRecord { level: LogLevel::Warn, span: vec!["updater".into()], message: "warn".into() },
        ]
    }

    fn check_round_trip<T>(value: &T) -> Result<(), String>
    where
        T: Serialize + DeserializeOwned + Clone + std::fmt::Debug + Into<TaggedEnvelope> + TryFrom<TaggedEnvelope, Error = TaggedDecodeError>,
    {
        let envelope: TaggedEnvelope = value.clone().into();
        let decoded: T = T::try_from(envelope.clone()).map_err(|err| err.to_string())?;
        let reencoded: TaggedEnvelope = decoded.clone().into();
        if envelope.kind != reencoded.kind || envelope.payload != reencoded.payload {
            return Err("tagged payload mismatch".into());
        }

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
            if let Err(err) = check_round_trip(&event) {
                panic!("updater event round-trip failed ({event:?}): {err}");
            }
        }
    }
}
