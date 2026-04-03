mod media;
mod options;
mod routes;
mod sidecar;

#[cfg(test)]
mod tests;

use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

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
pub(crate) struct ImuSidecarSession {
    pub(super) media_name: String,
    pub(super) sidecar_file_name: String,
    pub(super) sidecar_path: PathBuf,
    pub(super) cancel: CancellationToken,
    pub(super) join: tokio::task::JoinHandle<Result<ImuSidecarSummary, String>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ActiveRecordingSession {
    pub(super) media_name: String,
    pub(super) output_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ImuSidecarSummary {
    pub(super) samples: u64,
    pub(super) bytes: u64,
}

#[derive(Default)]
pub(crate) struct RecordingRuntimeState {
    imu_sidecar_sessions: Mutex<HashMap<Uuid, ImuSidecarSession>>,
    active_recording_sessions: Mutex<HashMap<Uuid, ActiveRecordingSession>>,
}

impl RecordingRuntimeState {
    pub(crate) async fn has_imu_sidecar_session(&self, stream_id: Uuid) -> bool {
        self.imu_sidecar_sessions.lock().await.contains_key(&stream_id)
    }

    pub(crate) async fn insert_imu_sidecar_session(&self, stream_id: Uuid, session: ImuSidecarSession) -> Result<(), ImuSidecarSession> {
        let mut sessions = self.imu_sidecar_sessions.lock().await;
        if sessions.contains_key(&stream_id) {
            return Err(session);
        }
        sessions.insert(stream_id, session);
        Ok(())
    }

    pub(crate) async fn remove_imu_sidecar_session(&self, stream_id: Uuid) -> Option<ImuSidecarSession> {
        self.imu_sidecar_sessions.lock().await.remove(&stream_id)
    }

    pub(crate) async fn insert_active_recording_session(&self, stream_id: Uuid, session: ActiveRecordingSession) {
        self.active_recording_sessions.lock().await.insert(stream_id, session);
    }

    pub(crate) async fn remove_active_recording_session(&self, stream_id: Uuid) -> Option<ActiveRecordingSession> {
        self.active_recording_sessions.lock().await.remove(&stream_id)
    }
}
