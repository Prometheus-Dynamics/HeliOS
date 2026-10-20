//! `POST /v1/playback` bodies and the JSON view of a player.

use serde::{Deserialize, Serialize};

use super::transport::PlaybackStatus;
use crate::error::{ApiError, ApiResult};

/// The slowest and fastest replay speeds (times the recorded rate). 0 is every frame as fast as
/// the consumers take them.
pub const SPEED_MIN: f32 = 0.25;
pub const SPEED_MAX: f32 = 4.0;
pub const DEFAULT_FPS: u32 = 30;
pub const FPS_MAX: u32 = 240;

/// What a player plays: a stored recording (by its id), or a file or folder under an allowed
/// folder (the recordings directory, or a configured root).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged, deny_unknown_fields)]
pub enum PlaybackSource {
    Recording { recording: String },
    Path { path: String },
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreatePlayback {
    pub source: PlaybackSource,
    /// Start again from the first frame at the end (default false).
    #[serde(default, rename = "loop")]
    pub looping: bool,
    /// Replay speed, 0.25 to 4 (default 1); 0 delivers every frame as fast as the consumers
    /// take them.
    #[serde(default)]
    pub speed: Option<f32>,
    /// Open paused on the first frame (default false).
    #[serde(default)]
    pub start_paused: bool,
    /// Image sequences only: frames per second (default 30).
    #[serde(default)]
    pub fps: Option<u32>,
}

impl CreatePlayback {
    /// The speed the player starts at.
    pub fn speed_or_default(&self) -> f32 {
        self.speed.unwrap_or(1.0)
    }

    pub fn validate(&self) -> ApiResult<()> {
        let speed = self.speed_or_default();
        if !(speed == 0.0 || (SPEED_MIN..=SPEED_MAX).contains(&speed)) || !speed.is_finite() {
            return Err(ApiError::bad_request(format!("speed must be 0 (every frame) or {SPEED_MIN} to {SPEED_MAX}, got {speed}")));
        }
        if let Some(fps) = self.fps
            && !(1..=FPS_MAX).contains(&fps)
        {
            return Err(ApiError::bad_request(format!("fps must be 1 to {FPS_MAX}, got {fps}")));
        }
        match &self.source {
            PlaybackSource::Recording { recording } if recording.trim().is_empty() => Err(ApiError::bad_request("source.recording is empty")),
            PlaybackSource::Path { path } if path.trim().is_empty() => Err(ApiError::bad_request("source.path is empty")),
            _ => Ok(()),
        }
    }
}

/// What kind of source a player replays.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    /// A recording (MCAP) with the replay controls.
    Recording,
    /// A folder of JPEG or PNG images, played at a fixed rate (no replay controls).
    Images,
}

/// A player as the API reports it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlayerView {
    pub id: String,
    /// The camera resource a pipeline binds to (`camera.playback.<token>`).
    pub resource_id: String,
    /// The label shown for it (`Playback: <recording id or file name>`).
    pub name: String,
    pub source: PlaybackSource,
    pub kind: SourceKind,
    /// A raw Bayer recording: its frames are processed by the software ISP (`ov9782`) as the camera
    /// would process them, so the player offers NV12 and pipelines bind to it like a live camera.
    pub raw: bool,
    /// What the player cannot do yet, for the UI to show (`null` when nothing).
    pub note: Option<String>,
    /// The camera service's socket (`styx-frames+unix://...`).
    pub frames_endpoint: String,
    pub speed: f32,
    #[serde(rename = "loop")]
    pub looping: bool,
    /// Image sequences: the frame rate.
    pub fps: Option<u32>,
    pub created_at_ms: u64,
    /// The replay state now; `null` with `status_error` when the service did not answer.
    pub status: Option<PlaybackStatus>,
    pub status_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> CreatePlayback {
        serde_json::from_str(json).expect("parses")
    }

    #[test]
    fn recording_and_path_sources_parse() {
        let by_id = parse(r#"{"source": {"recording": "rec-1"}, "loop": true, "speed": 0.5}"#);
        assert_eq!(by_id.source, PlaybackSource::Recording { recording: "rec-1".into() });
        assert!(by_id.looping);
        assert!(by_id.validate().is_ok());
        let by_path = parse(r#"{"source": {"path": "/var/lib/helios/images"}, "fps": 10}"#);
        assert_eq!(by_path.source, PlaybackSource::Path { path: "/var/lib/helios/images".into() });
        assert_eq!(by_path.speed_or_default(), 1.0);
        assert!(serde_json::from_str::<CreatePlayback>(r#"{"source": {"recording": "a", "path": "b"}}"#).is_err());
        assert!(serde_json::from_str::<CreatePlayback>(r#"{"source": {"path": "b"}, "bogus": 1}"#).is_err());
    }

    #[test]
    fn speed_is_zero_or_within_the_replay_range() {
        for speed in [0.0, 0.25, 1.0, 4.0] {
            let request = CreatePlayback { speed: Some(speed), ..parse(r#"{"source": {"path": "x"}}"#) };
            assert!(request.validate().is_ok(), "speed {speed}");
        }
        for speed in [0.1, 4.5, -1.0, f32::NAN] {
            let request = CreatePlayback { speed: Some(speed), ..parse(r#"{"source": {"path": "x"}}"#) };
            assert!(request.validate().is_err(), "speed {speed}");
        }
    }

    #[test]
    fn fps_and_empty_sources_are_checked() {
        assert!(CreatePlayback { fps: Some(0), ..parse(r#"{"source": {"path": "x"}}"#) }.validate().is_err());
        assert!(CreatePlayback { fps: Some(FPS_MAX + 1), ..parse(r#"{"source": {"path": "x"}}"#) }.validate().is_err());
        assert!(parse(r#"{"source": {"recording": "  "}}"#).validate().is_err());
    }
}
