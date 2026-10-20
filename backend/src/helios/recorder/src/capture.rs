//! The capture-last clip: a capture-last ring's frames written as a recording part (the same MCAP,
//! start records and summary as any recording), while the ring goes on running.

use std::{path::Path, sync::Arc};

use chrono::Utc;

use crate::{
    error::RecorderError,
    info::RecordingInfo,
    naming,
    ring::RingFrame,
    status::PartSummary,
    writer::{self, PartWriter},
};

/// Bytes per pixel of a frame format before compression (16-bit Bayer: two; luma: one).
pub fn bytes_per_pixel(code: styx::prelude::FourCc) -> u64 {
    if code.is_bayer_raw() { 2 } else { 1 }
}

/// What a clip is named after and written with.
pub struct ClipRequest<'a> {
    pub camera_id: &'a str,
    pub board: &'a str,
    pub camera_label: &'a str,
    pub commit: &'a str,
    pub info: &'a RecordingInfo,
    pub compression: bool,
    pub queue_bytes: usize,
    /// The recording rate the ring runs at (the clip's interval).
    pub max_fps: Option<f64>,
}

/// Writes `frames` (oldest first) as a new stored recording in `dir`, named
/// `<session>-last.mcap`, and returns its summary.
pub fn write_clip(frames: &[Arc<RingFrame>], dir: &Path, clip: &ClipRequest<'_>) -> Result<PartSummary, RecorderError> {
    let first = frames.first().ok_or_else(|| RecorderError::NotFound("the ring holds no frames yet".into()))?;
    let stem = naming::session_stem(clip.board, clip.camera_label, Utc::now(), clip.commit);
    let session = naming::unused_session_stem(dir, &format!("{stem}-last"));
    let size = (first.format.resolution.width.get(), first.format.resolution.height.get());
    let header = writer::header_for(clip.camera_id, first.format.code, size, clip.max_fps.map(|fps| fps as f32))?;
    let options = writer::options_for(clip.compression, clip.queue_bytes);
    let raw_frame = u64::from(size.0) * u64::from(size.1) * bytes_per_pixel(first.format.code);
    let mut part = PartWriter::open(dir, &session, clip.camera_id, 1, &header, options, clip.info)?;
    for frame in frames {
        part.wait_for_room(raw_frame);
        part.record(&frame.to_lease()?)?;
    }
    part.finish(Some("capture_last".into()), 0)
}
