//! The camera's frames for a recording: a low-priority client of the camera service on its
//! frames socket, so it never restarts or re-plans the capture the pipelines use. What it asks
//! for is the recording's `frames` setting: the sensor's raw frames (the default) or luma.

use std::{cell::RefCell, path::Path, time::Duration};

use styx::{
    core::prelude::{FrameLease, RecvOutcome},
    ipc::FrameClient,
    planner::{Delivered, FrameRequest, Frames},
};

use crate::{error::RecorderError, settings::RecordFrames};

/// How long opening the camera service's connection may take.
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(5);
/// Frames the camera service may queue for the recorder while the disk is busy (the recorder's own
/// queue is `RecorderSettings::queue_bytes`).
pub const CLIENT_QUEUE: usize = 8;

/// The frames a recording asks the camera service for, every frame (the recorder subsamples them
/// to `max_fps`). Raw is the sensor's 16-bit Bayer, a companion of the processed capture (no
/// copy); the camera service keeps it on, so asking for it never restarts the capture.
pub fn request(frames: RecordFrames) -> FrameRequest {
    match frames {
        RecordFrames::Raw => Frames::sensor_raw().every_frame(CLIENT_QUEUE),
        RecordFrames::Luma => Frames::gray().every_frame(CLIENT_QUEUE),
    }
}

/// A low-priority client of a camera service, and what its frames are.
pub struct FrameSource {
    client: FrameClient,
    delivered: Option<Delivered>,
    /// The frame read while opening, returned by the first [`FrameSource::next`].
    first: RefCell<Option<FrameLease>>,
}

impl FrameSource {
    /// Connects to the camera service at `socket` and asks for `request`. Blocks up to
    /// [`OPEN_TIMEOUT`] for the connection, and as long again for a first frame: its format and
    /// size are what the recording's header says (the service's `delivered` names the processed
    /// capture's format for a `sensor_raw` request, not the raw frames').
    pub fn open(socket: &Path, request: &FrameRequest) -> Result<Self, RecorderError> {
        let client = FrameClient::options(socket).low_priority().reconnecting().timeout(OPEN_TIMEOUT).request(request).map_err(|error| RecorderError::Camera(error.to_string()))?;
        let mut delivered = client.delivered();
        let first = match client.recv(OPEN_TIMEOUT) {
            RecvOutcome::Data(frame) => Some(frame),
            RecvOutcome::Empty => None,
            RecvOutcome::Closed => return Err(RecorderError::Camera(client.last_error().map(|e| e.to_string()).unwrap_or_else(|| "camera service closed".into()))),
        };
        if let (Some(delivered), Some(frame)) = (delivered.as_mut(), first.as_ref()) {
            let format = frame.meta().format;
            delivered.format = format.code;
            delivered.size = (format.resolution.width.get(), format.resolution.height.get());
        }
        Ok(Self { client, delivered, first: RefCell::new(first) })
    }

    pub fn delivered(&self) -> Option<&Delivered> {
        self.delivered.as_ref()
    }

    /// The next frame, waiting up to `wait`. `Err` only when the service is gone for good.
    pub fn next(&self, wait: Duration) -> Result<Option<FrameLease>, RecorderError> {
        if let Some(frame) = self.first.borrow_mut().take() {
            return Ok(Some(frame));
        }
        match self.client.recv(wait) {
            RecvOutcome::Data(frame) => Ok(Some(frame)),
            RecvOutcome::Empty => Ok(None),
            RecvOutcome::Closed => Err(RecorderError::Camera(self.client.last_error().map(|e| e.to_string()).unwrap_or_else(|| "camera service closed".into()))),
        }
    }

    /// Times the camera service connected again (the recorder keeps going across restarts).
    pub fn reconnects(&self) -> u64 {
        self.client.reconnects()
    }
}
