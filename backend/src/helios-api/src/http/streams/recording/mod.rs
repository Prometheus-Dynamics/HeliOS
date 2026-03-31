mod media;
mod options;
mod routes;
mod sidecar;

#[cfg(test)]
mod tests;

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use utoipa::ToSchema;
use uuid::Uuid;

pub(crate) use routes::*;

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct StartRecordingRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub fps: Option<f32>,
    #[serde(default)]
    pub bitrate_bps: Option<u64>,
    #[serde(default)]
    pub gop: Option<i32>,
    #[serde(default)]
    pub quality: Option<u8>,
    #[serde(default)]
    pub max_width: Option<u32>,
    #[serde(default)]
    pub max_height: Option<u32>,
    #[serde(default)]
    pub source: Option<helios_engine::ipc::RecordingSource>,
    #[serde(default)]
    pub include_imu: Option<bool>,
    #[serde(default)]
    pub imu_interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CaptureShadowRecordingRequest {
    pub window_ms: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
}

pub(super) const IMU_SIDE_CAR_DEFAULT_INTERVAL_MS: u64 = 20;
pub(super) const IMU_SIDE_CAR_MIN_INTERVAL_MS: u64 = 5;
pub(super) const IMU_SIDE_CAR_MAX_INTERVAL_MS: u64 = 2_000;
pub(super) const IMU_SIDE_CAR_HISTORY_WINDOW_MS: i64 = 10_000;
pub(super) const IMU_SIDE_CAR_HISTORY_MAX_SAMPLES: usize = 4_096;
pub(super) const IMU_SIDE_CAR_STOP_TAIL_IDLE_MS: u64 = 300;
pub(super) const IMU_SIDE_CAR_STOP_TAIL_MAX_MS: u64 = 5_000;
pub(super) const IMU_SIDE_CAR_STOP_WAIT_QUIET_MS: u64 = 250;
pub(super) const IMU_SIDE_CAR_STOP_WAIT_MAX_MS: u64 = 5_000;
pub(super) const RECORDING_STOP_GRACE_DEFAULT_MS: u64 = 0;
pub(super) const RECORDING_STOP_GRACE_MIN_MS: u64 = 0;
pub(super) const RECORDING_STOP_GRACE_MAX_MS: u64 = 2_000;

#[derive(Debug)]
pub(super) struct ImuSidecarSession {
    pub(super) media_name: String,
    pub(super) sidecar_file_name: String,
    pub(super) sidecar_path: PathBuf,
    pub(super) cancel: CancellationToken,
    pub(super) join: tokio::task::JoinHandle<Result<ImuSidecarSummary, String>>,
}

#[derive(Debug, Clone)]
pub(super) struct ActiveRecordingSession {
    pub(super) media_name: String,
    pub(super) output_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ImuSidecarSummary {
    pub(super) samples: u64,
    pub(super) bytes: u64,
}

pub(super) fn imu_sidecar_sessions() -> &'static Mutex<HashMap<Uuid, ImuSidecarSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<Uuid, ImuSidecarSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn active_recording_sessions() -> &'static Mutex<HashMap<Uuid, ActiveRecordingSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<Uuid, ActiveRecordingSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}
