//! Where a frame-driven workload's frames come from.
//!
//! Sources are polled from the workload's graph thread, never from a thread of their own: a Styx
//! `FrameClient` is pollable (Styx `docs/frame-server.md`, "Without a thread per client"), so the
//! graph thread awaits its camera's frames and the graph's inbound wake together.

use std::{
    task::{Context, Poll},
    time::{Duration, Instant},
};

use styx::{core::prelude::RecvOutcome, imports::framelease::FrameLease, ipc::FrameClient};

use super::bindings::FrameSourceSpec;

/// Backoff between attempts to reach a camera service that is not up yet.
const CONNECT_RETRY_MIN: Duration = Duration::from_millis(100);
const CONNECT_RETRY_MAX: Duration = Duration::from_secs(2);
/// How long opening a camera service may take (connect plus its answer) before the driver backs off.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

// A frame is moved once per receive; boxing it would only add an allocation per frame.
#[allow(clippy::large_enum_variant)]
pub(crate) enum FrameReceive {
    Frame(FrameLease),
    /// No frame now.
    Idle,
    /// The connection is gone for good (and why); the source has to connect again.
    Closed(String),
}

/// Connection details reported in workload stats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FrameSourceStatus {
    pub connected: bool,
    pub reconnects: u64,
    pub plan: Option<String>,
}

pub(crate) trait FrameSource: Send {
    /// Make sure the source has a connection (opening one may block for its timeout). When it
    /// has none: why, and how long until the next attempt is due.
    fn connect(&mut self) -> Result<(), (String, Duration)>;
    /// The next frame from a connected source: `Pending` registers `cx` to be woken when there
    /// is one (or news, such as the connection going away).
    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<FrameReceive>;
    /// The next frame if one is there, without waiting.
    fn try_frame(&mut self) -> FrameReceive;
    fn status(&self) -> FrameSourceStatus;
}

/// Frames requested from a Styx `CameraService`. The first request is retried with backoff
/// until the service answers; afterwards the client reconnects on its own, without blocking.
pub(crate) struct StyxFrameSource {
    spec: FrameSourceSpec,
    client: Option<FrameClient>,
    next_attempt: Instant,
    backoff: Duration,
    last_error: Option<String>,
}

impl StyxFrameSource {
    pub fn new(spec: FrameSourceSpec) -> Self {
        Self { spec, client: None, next_attempt: Instant::now(), backoff: CONNECT_RETRY_MIN, last_error: None }
    }

    fn closed(&mut self, why: &str) -> FrameReceive {
        self.client = None;
        FrameReceive::Closed(format!("camera service '{}' {why}", self.spec.socket_path.display()))
    }
}

impl FrameSource for StyxFrameSource {
    fn connect(&mut self) -> Result<(), (String, Duration)> {
        if self.client.is_some() {
            return Ok(());
        }
        let until_attempt = self.next_attempt.saturating_duration_since(Instant::now());
        if !until_attempt.is_zero() {
            return Err((self.last_error.clone().unwrap_or_else(|| "camera service not connected".into()), until_attempt));
        }
        let mut options = FrameClient::options(&self.spec.socket_path).timeout(CONNECT_TIMEOUT).reconnecting();
        if let Some(camera) = &self.spec.request.camera {
            options = options.camera(camera);
        }
        match options.request(&self.spec.request()) {
            Ok(client) => {
                tracing::info!(resource_id = %self.spec.resource_id, socket = %self.spec.socket_path.display(), delivered = ?client.delivered(), plan = client.plan().as_deref().unwrap_or(""), "connected to camera service");
                self.client = Some(client);
                self.backoff = CONNECT_RETRY_MIN;
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                let message = format!("camera service '{}' for resource '{}': {error}", self.spec.socket_path.display(), self.spec.resource_id);
                let retry_in = self.backoff;
                self.next_attempt = Instant::now() + retry_in;
                self.backoff = (self.backoff * 2).min(CONNECT_RETRY_MAX);
                self.last_error = Some(message.clone());
                Err((message, retry_in))
            }
        }
    }

    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<FrameReceive> {
        let Some(client) = &self.client else {
            return Poll::Ready(FrameReceive::Closed("camera service not connected".into()));
        };
        match client.poll_next(cx) {
            Poll::Ready(RecvOutcome::Data(frame)) => Poll::Ready(FrameReceive::Frame(frame)),
            Poll::Ready(RecvOutcome::Empty) => Poll::Ready(FrameReceive::Idle),
            Poll::Ready(RecvOutcome::Closed) => Poll::Ready(self.closed("closed the connection")),
            Poll::Pending => Poll::Pending,
        }
    }

    fn try_frame(&mut self) -> FrameReceive {
        if self.client.is_none() && self.connect().is_err() {
            return FrameReceive::Idle;
        }
        let Some(client) = &self.client else {
            return FrameReceive::Idle;
        };
        match client.try_next() {
            RecvOutcome::Data(frame) => FrameReceive::Frame(frame),
            RecvOutcome::Empty => FrameReceive::Idle,
            RecvOutcome::Closed => self.closed("closed the connection"),
        }
    }

    fn status(&self) -> FrameSourceStatus {
        match &self.client {
            Some(client) => FrameSourceStatus { connected: client.is_connected(), reconnects: client.reconnects(), plan: client.plan() },
            None => FrameSourceStatus::default(),
        }
    }
}
