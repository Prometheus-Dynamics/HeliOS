use serde::Deserialize;
use utoipa::ToSchema;

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
