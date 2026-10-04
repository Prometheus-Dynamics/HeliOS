//! Where a frame-driven workload's frames come from.

use std::time::{Duration, Instant};

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
    /// No frame within the wait.
    Idle,
    /// The source cannot deliver frames right now (and why).
    Unavailable(String),
}

/// Connection details reported in workload stats.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FrameSourceStatus {
    pub connected: bool,
    pub reconnects: u64,
    pub plan: Option<String>,
}

pub(crate) trait FrameSource: Send {
    /// The next frame, waiting at most `wait`.
    fn recv(&mut self, wait: Duration) -> FrameReceive;
    fn status(&self) -> FrameSourceStatus;
}

/// Frames requested from a Styx `CameraService`. The request is made lazily and retried with
/// backoff until the service answers; afterwards the client reconnects on its own.
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

    fn connect(&mut self) -> Result<(), String> {
        let mut options = FrameClient::options(&self.spec.socket_path).timeout(CONNECT_TIMEOUT);
        if let Some(camera) = &self.spec.request.camera {
            options = options.camera(camera);
        }
        let client = options.request(&self.spec.request());
        match client {
            Ok(client) => {
                tracing::info!(resource_id = %self.spec.resource_id, socket = %self.spec.socket_path.display(), delivered = ?client.delivered(), plan = client.plan().as_deref().unwrap_or(""), "connected to camera service");
                self.client = Some(client.reconnecting());
                self.backoff = CONNECT_RETRY_MIN;
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                let message = format!("camera service '{}' for resource '{}': {error}", self.spec.socket_path.display(), self.spec.resource_id);
                self.next_attempt = Instant::now() + self.backoff;
                self.backoff = (self.backoff * 2).min(CONNECT_RETRY_MAX);
                self.last_error = Some(message.clone());
                Err(message)
            }
        }
    }
}

impl FrameSource for StyxFrameSource {
    fn recv(&mut self, wait: Duration) -> FrameReceive {
        if self.client.is_none() {
            let until_attempt = self.next_attempt.saturating_duration_since(Instant::now());
            if !until_attempt.is_zero() {
                std::thread::sleep(until_attempt.min(wait));
                return FrameReceive::Unavailable(self.last_error.clone().unwrap_or_else(|| "camera service not connected".into()));
            }
            if let Err(error) = self.connect() {
                return FrameReceive::Unavailable(error);
            }
        }
        let Some(client) = &self.client else {
            return FrameReceive::Unavailable("camera service not connected".into());
        };
        match client.recv(wait) {
            RecvOutcome::Data(frame) => FrameReceive::Frame(frame),
            RecvOutcome::Empty if client.is_connected() => FrameReceive::Idle,
            RecvOutcome::Empty => FrameReceive::Unavailable(format!("camera service '{}' disconnected; reconnecting", self.spec.socket_path.display())),
            RecvOutcome::Closed => {
                self.client = None;
                FrameReceive::Unavailable(format!("camera service '{}' closed the connection", self.spec.socket_path.display()))
            }
        }
    }

    fn status(&self) -> FrameSourceStatus {
        match &self.client {
            Some(client) => FrameSourceStatus { connected: client.is_connected(), reconnects: client.reconnects(), plan: client.plan() },
            None => FrameSourceStatus::default(),
        }
    }
}
