//! The replay controls of a player (Styx `replay_controls`, by name): the status the API reports,
//! and the writes a seek makes. Transport (play, pause, speed, loop, step) is set through the
//! camera settings API, which takes the same names; this module only reads state and maps seeks.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::error::{ApiError, ApiResult};

pub const CONTROL_PLAYING: &str = "replay_playing";
pub const CONTROL_SPEED: &str = "replay_speed";
pub const CONTROL_LOOP: &str = "replay_loop";
pub const CONTROL_SEEK_MS: &str = "replay_seek_ms";
pub const CONTROL_SEEK_FRAME: &str = "replay_seek_frame";
pub const CONTROL_STEP: &str = "replay_step";
pub const CONTROL_POSITION_MS: &str = "replay_position_ms";
pub const CONTROL_FRAME: &str = "replay_frame";
pub const CONTROL_DURATION_MS: &str = "replay_duration_ms";
pub const CONTROL_FRAME_COUNT: &str = "replay_frame_count";
pub const CONTROL_STATE: &str = "replay_state";

/// The replay controls' names, as Styx lists them (the transport controls of a player).
pub fn is_transport_control(name: &str) -> bool {
    name.starts_with("replay_")
}

/// A player's replay state now.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlaybackStatus {
    /// `playing`, `paused`, `ended`, or `unknown` when the state did not read.
    pub state: &'static str,
    pub position_ms: u64,
    /// The last frame delivered, from 0 within the pass.
    pub frame: u64,
    pub duration_ms: u64,
    pub frame_count: u64,
    pub speed: Option<f64>,
    #[serde(rename = "loop")]
    pub looping: bool,
}

/// The status from the controls' current values, by control name (a missing or unreadable value
/// is `None`).
pub fn status_from_values(values: &BTreeMap<String, Value>) -> PlaybackStatus {
    let number = |name: &str| values.get(name).and_then(Value::as_u64).unwrap_or(0);
    let state = match values.get(CONTROL_STATE).and_then(Value::as_u64) {
        Some(0) => "playing",
        Some(1) => "paused",
        Some(2) => "ended",
        _ => "unknown",
    };
    PlaybackStatus {
        state,
        position_ms: number(CONTROL_POSITION_MS),
        frame: number(CONTROL_FRAME),
        duration_ms: number(CONTROL_DURATION_MS),
        frame_count: number(CONTROL_FRAME_COUNT),
        speed: values.get(CONTROL_SPEED).and_then(Value::as_f64),
        looping: values.get(CONTROL_LOOP).and_then(Value::as_bool).unwrap_or(false),
    }
}

/// Where a seek goes: a time from the first frame, or a frame of the pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seek {
    Ms(u64),
    Frame(u64),
}

/// `{"ms": 1500}` or `{"frame": 42}`, not both.
#[derive(Debug, Clone, Copy, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SeekRequest {
    #[serde(default)]
    pub ms: Option<u64>,
    #[serde(default)]
    pub frame: Option<u64>,
}

impl SeekRequest {
    pub fn seek(self) -> ApiResult<Seek> {
        match (self.ms, self.frame) {
            (Some(ms), None) => Ok(Seek::Ms(ms)),
            (None, Some(frame)) => Ok(Seek::Frame(frame)),
            _ => Err(ApiError::bad_request("give a seek as {\"ms\": <time>} or {\"frame\": <frame>}, one of them")),
        }
    }
}

/// The control write a seek makes, as the settings API takes it (`{name: value}`).
pub fn seek_change(seek: Seek) -> (&'static str, Value) {
    match seek {
        Seek::Ms(ms) => (CONTROL_SEEK_MS, Value::from(ms)),
        Seek::Frame(frame) => (CONTROL_SEEK_FRAME, Value::from(frame)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn values(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
        pairs.iter().map(|(name, value)| (name.to_string(), value.clone())).collect()
    }

    #[test]
    fn status_reads_every_replay_control() {
        let status = status_from_values(&values(&[
            (CONTROL_STATE, json!(1)),
            (CONTROL_POSITION_MS, json!(1500)),
            (CONTROL_FRAME, json!(45)),
            (CONTROL_DURATION_MS, json!(60000)),
            (CONTROL_FRAME_COUNT, json!(1800)),
            (CONTROL_SPEED, json!(2.0)),
            (CONTROL_LOOP, json!(true)),
        ]));
        assert_eq!(status.state, "paused");
        assert_eq!((status.position_ms, status.frame, status.duration_ms, status.frame_count), (1500, 45, 60000, 1800));
        assert_eq!((status.speed, status.looping), (Some(2.0), true));
    }

    #[test]
    fn status_states_map_and_missing_values_read_as_unknown_or_zero() {
        assert_eq!(status_from_values(&values(&[(CONTROL_STATE, json!(0))])).state, "playing");
        assert_eq!(status_from_values(&values(&[(CONTROL_STATE, json!(2))])).state, "ended");
        let empty = status_from_values(&BTreeMap::new());
        assert_eq!((empty.state, empty.position_ms, empty.speed, empty.looping), ("unknown", 0, None, false));
        assert_eq!(status_from_values(&values(&[(CONTROL_STATE, json!(9))])).state, "unknown");
    }

    #[test]
    fn seek_requests_take_exactly_one_target() {
        assert_eq!(SeekRequest { ms: Some(1500), frame: None }.seek().expect("ms"), Seek::Ms(1500));
        assert_eq!(SeekRequest { ms: None, frame: Some(0) }.seek().expect("frame"), Seek::Frame(0));
        assert!(SeekRequest { ms: Some(1), frame: Some(2) }.seek().is_err());
        assert!(SeekRequest { ms: None, frame: None }.seek().is_err());
    }

    #[test]
    fn seeks_write_the_styx_seek_controls() {
        assert_eq!(seek_change(Seek::Ms(250)), (CONTROL_SEEK_MS, json!(250)));
        assert_eq!(seek_change(Seek::Frame(7)), (CONTROL_SEEK_FRAME, json!(7)));
        assert!(is_transport_control(CONTROL_SPEED));
        assert!(!is_transport_control("ae"));
    }
}
