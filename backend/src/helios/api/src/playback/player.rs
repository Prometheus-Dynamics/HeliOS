//! One player: the Styx device for its source (a replayed recording, or an image sequence), and
//! the `CameraService` that serves it on the player's socket.

use std::path::{Path, PathBuf};

use styx::{
    ProbedDevice,
    capture_api::CaptureRequest,
    ipc::{CameraService, CameraServiceHandle},
    replay::{ReplayPacing, ReplaySourceConfig, ReplayTimestamps},
};

use super::{
    facts::RecordingFacts,
    request::{PlayerView, SourceKind},
    source::Resolved,
};
use crate::error::{ApiError, ApiResult};

/// The built-in sensor description and tuning that process a raw recording's frames (the OV9782
/// of the Raze camera).
pub const ISP_SENSOR: &str = "ov9782";

/// The socket file of player `token`, in the stream directory the engine reaches.
pub fn socket_path(stream_dir: &Path, token: &str) -> PathBuf {
    stream_dir.join(format!("playback-{token}.styx.sock"))
}

/// What a player starts with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartOptions {
    pub looping: bool,
    /// 0: every frame as fast as the consumers take them.
    pub speed: f32,
    /// Image sequences: frames per second.
    pub fps: u32,
}

/// The Styx device for `resolved`: a replayed recording held at its last frame (a player is
/// kept open for seeks and loops), or an image sequence.
pub fn device_for(resolved: &Resolved, options: StartOptions) -> ApiResult<ProbedDevice> {
    match resolved.kind {
        SourceKind::Recording | SourceKind::Images => {
            let path = resolved.recording.as_ref().ok_or_else(|| ApiError::internal("a recording source without its file"))?;
            // Speed 0 replays unpaced (every frame as fast as the consumers take them); any other
            // speed starts realtime and is set through `replay_speed` once the player is open.
            let pacing = if options.speed == 0.0 { ReplayPacing::Unpaced } else { ReplayPacing::Realtime };
            // The frame rate is for videos without one (Motion JPEG streams, JPEG folders).
            let mut config = ReplaySourceConfig::new(path).hold_at_end(true).timestamps(ReplayTimestamps::Rebased).loop_forever(options.looping).pacing(pacing).frame_rate(options.fps);
            // Raw Bayer frames go through the software ISP as the camera would process them (the
            // recorded exposure, gains and white balance; no AE or AWB), so the player offers NV12.
            if resolved.raw {
                config = config.soft_isp(ISP_SENSOR);
            }
            let source = CaptureRequest::replay_source(config).map_err(|error| ApiError::unprocessable(format!("replay: {error}")))?;
            Ok(source.into_device())
        }
    }
}

/// Serves `device` on `socket` (replacing a stale socket file). Blocking: run it off the async
/// runtime.
pub fn serve(socket: &Path, device: ProbedDevice) -> ApiResult<CameraServiceHandle> {
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir).map_err(|error| ApiError::internal(format!("creating {}: {error}", dir.display())))?;
    }
    CameraService::new(device).socket_mode(0o660).serve(socket).map_err(|error| ApiError::backend(format!("camera service for {}: {error}", socket.display())))
}

/// An open player: its view, its socket, and the service (dropped when it closes).
pub struct Player {
    pub view: PlayerView,
    pub token: String,
    pub socket: PathBuf,
    /// The recording's start-time facts (a run takes its calibration and mount from them).
    pub facts: RecordingFacts,
    pub(super) service: Option<CameraServiceHandle>,
}

impl Player {
    /// Stops the service and removes its socket. Blocking (the service joins its threads).
    pub async fn stop(mut self) {
        if let Some(service) = self.service.take() {
            let _ = tokio::task::spawn_blocking(move || drop(service)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sockets_are_named_by_token_in_the_stream_directory() {
        let path = socket_path(Path::new("/run/helios/streams"), "1a2b3c4d");
        assert_eq!(path, PathBuf::from("/run/helios/streams/playback-1a2b3c4d.styx.sock"));
    }
}
