//! Capture-last rings in helios-api: the cameras whose settings ask for a ring (`capture_last_cameras`)
//! keep one running. A supervisor makes the rings match the settings and the cameras present every
//! few seconds (a camera coming back gets its ring, a failed ring is tried again), and the settings
//! and the clip route make them match at once. The rings themselves are helios-recorder's.

use std::time::Duration;

use helios_recorder::{RingRequest, RingStatus};
use tokio::task::JoinHandle;
use tracing::warn;

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    routes::cameras::{self, frames_socket},
};

/// How often the rings are checked against the settings and the cameras present.
const SUPERVISE_EVERY: Duration = Duration::from_secs(5);

/// The rings the settings ask for, with the camera services present now (a camera that is not
/// there has none; its ring stops).
pub async fn wanted(state: &SharedState) -> ApiResult<Vec<RingRequest>> {
    let settings = state.store.recording_settings().await?;
    let mut requests = Vec::new();
    for camera in &settings.capture_last_cameras {
        let Ok((_, record)) = cameras::find(state, camera).await else { continue };
        let Some(socket) = frames_socket(&record.endpoints) else { continue };
        requests.push(RingRequest { camera_id: camera.clone(), frames_socket: socket, settings: settings.clone() });
    }
    Ok(requests)
}

/// Makes the rings match the settings now. The camera services are opened on a worker thread.
pub async fn sync(state: &SharedState) -> ApiResult<Vec<RingStatus>> {
    let requests = wanted(state).await?;
    let state = state.clone();
    tokio::task::spawn_blocking(move || state.recorder.sync_rings(&requests)).await.map_err(|error| ApiError::internal(error.to_string()))
}

/// Keeps the rings matching the settings and the cameras, for as long as the API runs.
pub fn spawn_supervisor(state: SharedState) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SUPERVISE_EVERY);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(error) = sync(&state).await {
                warn!(error = %error.message, "capture-last rings not synced");
            }
        }
    })
}

/// Syncs the rings in the background (after the settings change: the caller does not wait for the
/// cameras to open).
pub fn sync_in_background(state: SharedState) {
    tokio::spawn(async move {
        if let Err(error) = sync(&state).await {
            warn!(error = %error.message, "capture-last rings not synced");
        }
    });
}
