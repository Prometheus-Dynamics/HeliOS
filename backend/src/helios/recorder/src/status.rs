//! What a recording reports: its live status, and the summary of each finished part (also kept
//! beside the part as `<stem>.summary.json`, so stored recordings list with their duration and
//! drops without reading the MCAP).

use serde::{Deserialize, Serialize};

use crate::settings::RecordFrames;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingState {
    /// Opened, first frame not yet written.
    Starting,
    Recording,
    /// Stop requested: the current part is being completed; frames are still read and dropped
    /// until it is, so the camera never waits on the recorder.
    Finishing,
    /// Stopped on request, or when a cap or the reserve ended it (`stop_reason`).
    Stopped,
    /// Ended by an error (`error`).
    Failed,
}

impl RecordingState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Recording | Self::Finishing)
    }
}

/// One finished part of a recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PartSummary {
    /// The session (recording) this part belongs to: its first part's stem.
    pub session: String,
    pub camera: String,
    pub file: String,
    /// 1 for the first part, then 2, 3...
    pub index: u32,
    /// Unix milliseconds the part was opened.
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub bytes: u64,
    pub frames_written: u64,
    pub frames_dropped: u64,
    pub messages_written: u64,
    /// Host messages lost: at the recorder's queue, or before it (the channel from the API).
    pub messages_dropped: u64,
    pub stop_reason: Option<String>,
}

/// A recording's live status: the session, its current part and the totals so far.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingStatus {
    /// The session id (the first part's file stem).
    pub id: String,
    pub camera: String,
    pub state: RecordingState,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub dir: String,
    /// The part being written (its file name), while the recording is active.
    pub file: Option<String>,
    /// Finished parts of this session.
    pub parts: Vec<PartSummary>,
    /// Bytes on disk over all parts, the current one included.
    pub bytes: u64,
    pub frames_written: u64,
    pub frames_dropped: u64,
    pub messages_written: u64,
    pub messages_dropped: u64,
    pub free_bytes: Option<u64>,
    pub reserve_bytes: u64,
    /// The recordings folder's budget now (the effective `max_total_bytes`).
    pub max_total_bytes: u64,
    pub max_fps: Option<f64>,
    /// What the frames are (`raw` or `luma`), and the delivered format and size (`BYR2 1280x800`).
    pub frames: RecordFrames,
    pub format: String,
    pub compression: bool,
    /// Why it stopped: `requested`, `storage_reserve`, `camera_lost`.
    pub stop_reason: Option<String>,
    pub error: Option<String>,
}
