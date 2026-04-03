use std::collections::HashMap;
use std::path::PathBuf;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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
