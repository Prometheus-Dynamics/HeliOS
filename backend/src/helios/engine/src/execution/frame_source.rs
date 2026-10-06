//! Where a frame-driven workload's frames come from.
//!
//! Sources are polled from the workload's graph thread, never from a thread of their own: a Styx
//! `FrameClient` is a file descriptor, readable when `try_next()` has a frame or news (Styx
//! `docs/frame-server.md`, "Without a thread per client"), so the graph thread waits on every
//! camera and on the graph's inbound fd in one `poll(2)`.

use std::{
    os::fd::{AsFd, BorrowedFd},
    time::Duration,
};

use styx::{core::prelude::RecvOutcome, imports::framelease::FrameLease, ipc::FrameClient};

use super::bindings::FrameSourceSpec;

/// How long one connection attempt (connect plus the service's answer) may take before the
/// client backs off and tries again (Styx backs off from 100 ms to 2 s between attempts).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

// A frame is moved once per receive; boxing it would only add an allocation per frame.
#[allow(clippy::large_enum_variant)]
pub(crate) enum FrameReceive {
    Frame(FrameLease),
    /// No frame now (also while the source is still connecting or reconnecting).
    Idle,
    /// The source gave up (and why); it has to be reopened.
    Closed(String),
}

/// Connection details reported in workload stats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FrameSourceStatus {
    pub connected: bool,
    pub reconnects: u64,
    pub plan: Option<String>,
    /// Why the last connection attempt failed, while not connected.
    pub error: Option<String>,
}

pub(crate) trait FrameSource: Send {
    /// The descriptor the graph thread polls: readable when [`FrameSource::try_frame`] has a
    /// frame or news (connected, the next connection attempt due, closed).
    fn as_fd(&self) -> BorrowedFd<'_>;
    /// The next frame if one is there, without waiting (connecting and reconnecting here, also
    /// without waiting).
    fn try_frame(&self) -> FrameReceive;
    fn status(&self) -> FrameSourceStatus;
    /// Start over after [`FrameReceive::Closed`].
    fn reopen(&mut self) -> Result<(), String>;
}

/// Frames requested from a Styx `CameraService` with a reconnecting, non-blocking client: it
/// is created at once, before the service answers (or exists), connects in the background and
/// comes back after the service restarts, so a camera that is not up never blocks the graph
/// thread.
pub(crate) struct StyxFrameSource {
    spec: FrameSourceSpec,
    client: FrameClient,
}

impl StyxFrameSource {
    /// Fails only when the client's descriptors cannot be made.
    pub fn open(spec: FrameSourceSpec) -> Result<Self, String> {
        let client = Self::client(&spec)?;
        Ok(Self { spec, client })
    }

    fn client(spec: &FrameSourceSpec) -> Result<FrameClient, String> {
        let mut options = FrameClient::options(&spec.socket_path).timeout(CONNECT_TIMEOUT).reconnecting();
        if let Some(camera) = &spec.request.camera {
            options = options.camera(camera);
        }
        options.request_nonblocking(&spec.request()).map_err(|error| format!("camera service '{}' for resource '{}': {error}", spec.socket_path.display(), spec.resource_id))
    }
}

impl FrameSource for StyxFrameSource {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.client.as_fd()
    }

    fn try_frame(&self) -> FrameReceive {
        match self.client.try_next() {
            RecvOutcome::Data(frame) => FrameReceive::Frame(frame),
            RecvOutcome::Empty => FrameReceive::Idle,
            RecvOutcome::Closed => {
                let why = self.client.last_error().map_or_else(|| "closed the connection".to_string(), |error| error.to_string());
                FrameReceive::Closed(format!("camera service '{}': {why}", self.spec.socket_path.display()))
            }
        }
    }

    fn status(&self) -> FrameSourceStatus {
        let connected = self.client.is_connected();
        FrameSourceStatus {
            connected,
            reconnects: self.client.reconnects(),
            plan: self.client.plan(),
            error: (!connected)
                .then(|| self.client.last_error().map(|error| format!("camera service '{}' for resource '{}': {error}", self.spec.socket_path.display(), self.spec.resource_id)))
                .flatten(),
        }
    }

    fn reopen(&mut self) -> Result<(), String> {
        self.client = Self::client(&self.spec)?;
        Ok(())
    }
}
