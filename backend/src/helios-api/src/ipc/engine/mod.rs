mod dispatcher;
mod rpc;
mod timeouts;
mod transport;

#[cfg(test)]
mod tests;

use std::{
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use helios_engine::ipc::{EngineCommand, EngineEvent};
use lib_ipc::client::{Client as GenericClient, Session as GenericSession, TransportConfig};
use lib_ipc::types::{CommandId, FeatureSet};
use lib_ipc::wire::ServiceKind;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{Duration, Instant};

use super::command_id_from_context;
use timeouts::{engine_request_send_timeout, scale_timeout, timeout_scale_for_streams};
#[cfg(test)]
pub(crate) use transport::connect_engine_at_for_tests;
pub use transport::connect_engine_best_effort;

pub type EngineClient = GenericClient<EngineClientConfig, EngineCommand, EngineEvent>;
pub type EngineSession = GenericSession<EngineCommand, EngineEvent>;

#[derive(Debug, Clone)]
pub struct EngineClientConfig {
    socket_path: PathBuf,
    journal_path: PathBuf,
    protocol: lib_ipc::types::ProtocolVersion,
    client_name: String,
    client_version: String,
    features: FeatureSet,
}

impl EngineClientConfig {
    pub fn new(socket_path: impl Into<PathBuf>, journal_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
            journal_path: journal_path.into(),
            protocol: lib_ipc::types::ProtocolVersion::default(),
            client_name: "helios-engine-client".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            features: FeatureSet::default(),
        }
    }
}

impl TransportConfig for EngineClientConfig {
    fn socket_path(&self) -> &std::path::Path {
        &self.socket_path
    }

    fn journal_path(&self) -> &std::path::Path {
        &self.journal_path
    }

    fn protocol(&self) -> lib_ipc::types::ProtocolVersion {
        self.protocol
    }

    fn client_name(&self) -> &str {
        &self.client_name
    }

    fn client_version(&self) -> &str {
        &self.client_version
    }

    fn features(&self) -> &FeatureSet {
        &self.features
    }

    fn service_kind(&self) -> ServiceKind {
        ServiceKind::Engine
    }
}

#[derive(Debug, Clone)]
pub struct StartRecordingParams {
    pub source: helios_engine::ipc::RecordingSource,
    pub output_path: String,
    pub container: helios_engine::ipc::RecordingContainer,
    pub codec: helios_engine::ipc::RecordingCodec,
    pub duration_ms: Option<u64>,
    pub settings: Option<helios_engine::ipc::RecordingSettings>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct EngineConnectionObservabilitySnapshot {
    pub connected: bool,
    pub last_disconnect_ms: Option<u64>,
    pub request_queue_capacity: usize,
    pub request_queue_depth: usize,
    pub request_queue_high_water: usize,
    pub pending_requests: usize,
    pub pending_requests_high_water: usize,
    pub connect_count: u64,
    pub disconnect_count: u64,
    pub request_send_timeouts: u64,
    pub request_send_failures: u64,
    pub request_timeouts: u64,
    pub disconnected_pending_requests: u64,
    pub completed_roundtrips: u64,
    pub roundtrip_total_ms: u64,
    pub roundtrip_max_ms: u64,
    pub unsolicited_events: u64,
    pub stale_response_drops: u64,
    pub no_subscriber_event_drops: u64,
    pub event_subscribers: u64,
    pub connect_event_subscribers: u64,
    pub active_streams: usize,
    pub timeout_scale_ppm: u64,
}

#[derive(Debug)]
struct EngineConnectionMetrics {
    request_queue_capacity: usize,
    request_queue_depth: AtomicUsize,
    request_queue_high_water: AtomicUsize,
    pending_requests: AtomicUsize,
    pending_requests_high_water: AtomicUsize,
    connect_count: AtomicU64,
    disconnect_count: AtomicU64,
    request_send_timeouts: AtomicU64,
    request_send_failures: AtomicU64,
    request_timeouts: AtomicU64,
    disconnected_pending_requests: AtomicU64,
    completed_roundtrips: AtomicU64,
    roundtrip_total_ms: AtomicU64,
    roundtrip_max_ms: AtomicU64,
    unsolicited_events: AtomicU64,
    stale_response_drops: AtomicU64,
    no_subscriber_event_drops: AtomicU64,
}

impl EngineConnectionMetrics {
    fn new(request_queue_capacity: usize) -> Self {
        Self {
            request_queue_capacity,
            request_queue_depth: AtomicUsize::new(0),
            request_queue_high_water: AtomicUsize::new(0),
            pending_requests: AtomicUsize::new(0),
            pending_requests_high_water: AtomicUsize::new(0),
            connect_count: AtomicU64::new(0),
            disconnect_count: AtomicU64::new(0),
            request_send_timeouts: AtomicU64::new(0),
            request_send_failures: AtomicU64::new(0),
            request_timeouts: AtomicU64::new(0),
            disconnected_pending_requests: AtomicU64::new(0),
            completed_roundtrips: AtomicU64::new(0),
            roundtrip_total_ms: AtomicU64::new(0),
            roundtrip_max_ms: AtomicU64::new(0),
            unsolicited_events: AtomicU64::new(0),
            stale_response_drops: AtomicU64::new(0),
            no_subscriber_event_drops: AtomicU64::new(0),
        }
    }

    fn record_queue_enqueue(&self) {
        let depth = self.request_queue_depth.fetch_add(1, Ordering::Relaxed) + 1;
        update_high_water(&self.request_queue_high_water, depth);
    }

    fn record_queue_dequeue(&self) {
        let _ = self.request_queue_depth.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| Some(current.saturating_sub(1)));
    }

    fn record_queue_timeout(&self) {
        self.request_send_timeouts.fetch_add(1, Ordering::Relaxed);
        update_high_water(&self.request_queue_high_water, self.request_queue_capacity);
    }

    fn record_queue_send_failure(&self) {
        self.request_send_failures.fetch_add(1, Ordering::Relaxed);
    }

    fn set_pending_len(&self, len: usize) {
        self.pending_requests.store(len, Ordering::Relaxed);
        update_high_water(&self.pending_requests_high_water, len);
    }

    fn record_connect(&self) {
        self.connect_count.fetch_add(1, Ordering::Relaxed);
    }

    fn record_disconnect(&self, dropped_pending: usize) {
        self.disconnect_count.fetch_add(1, Ordering::Relaxed);
        self.disconnected_pending_requests.fetch_add(dropped_pending as u64, Ordering::Relaxed);
        self.set_pending_len(0);
    }

    fn record_request_timeout(&self, count: usize) {
        self.request_timeouts.fetch_add(count as u64, Ordering::Relaxed);
    }

    fn record_roundtrip(&self, elapsed: Duration) {
        let elapsed_ms = elapsed.as_millis().min(u64::MAX as u128) as u64;
        self.completed_roundtrips.fetch_add(1, Ordering::Relaxed);
        self.roundtrip_total_ms.fetch_add(elapsed_ms, Ordering::Relaxed);
        let mut current = self.roundtrip_max_ms.load(Ordering::Relaxed);
        while elapsed_ms > current {
            match self.roundtrip_max_ms.compare_exchange(current, elapsed_ms, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(observed) => current = observed,
            }
        }
    }

    fn record_unsolicited_event(&self) {
        self.unsolicited_events.fetch_add(1, Ordering::Relaxed);
    }

    fn record_stale_response_drop(&self) {
        self.stale_response_drops.fetch_add(1, Ordering::Relaxed);
    }

    fn record_no_subscriber_event_drop(&self) {
        self.no_subscriber_event_drops.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub struct EngineConnection {
    requests: mpsc::Sender<EngineRequest>,
    request_queue_capacity: usize,
    /// Fan-out for unsolicited engine events so callers can observe telemetry without blocking RPCs.
    events: broadcast::Sender<EngineEvent>,
    connect_events: broadcast::Sender<()>,
    connected: Arc<AtomicBool>,
    last_disconnect_ms: Arc<AtomicU64>,
    timeout_scale_ppm: Arc<AtomicU64>,
    active_streams: Arc<AtomicUsize>,
    metrics: Arc<EngineConnectionMetrics>,
}

impl EngineConnection {
    fn timeout_scale_ppm(&self) -> u64 {
        self.timeout_scale_ppm.load(Ordering::Relaxed)
    }

    fn update_timeout_scale_from_count(&self, count: usize) {
        self.active_streams.store(count, Ordering::Relaxed);
        self.timeout_scale_ppm.store(timeout_scale_for_streams(count), Ordering::Relaxed);
    }

    fn scaled_timeout(&self, base: Duration) -> Duration {
        scale_timeout(base, self.timeout_scale_ppm())
    }

    async fn request<F>(&self, make_command: F, expected: ExpectedEvent, label: &'static str, timeout: Duration) -> Result<EngineEvent, lib_ipc::client::ClientTransportError>
    where
        F: FnOnce(CommandId) -> EngineCommand,
    {
        let timeout = self.scaled_timeout(timeout);
        let command_id = command_id_from_context(label);
        let command = make_command(command_id);
        let journal_mode = JournalMode::for_command(&command);
        let (tx, rx) = oneshot::channel();
        let issued_at = Instant::now();
        let msg = EngineRequest { command_id, command, expected, respond_to: tx, label, issued_at, deadline: issued_at + timeout, saw_transport_ack: false, journal_mode };
        let send_timeout = self.scaled_timeout(engine_request_send_timeout());
        match tokio::time::timeout(send_timeout, self.requests.send(msg)).await {
            Ok(Ok(())) => {
                self.metrics.record_queue_enqueue();
            }
            Ok(Err(_)) => {
                self.metrics.record_queue_send_failure();
                return Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "engine dispatcher stopped")));
            }
            Err(_) => {
                self.metrics.record_queue_timeout();
                return Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::TimedOut, "engine request queue is full")));
            }
        }
        rx.await.unwrap_or_else(|_| Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "engine dispatcher dropped response"))))
    }

    pub fn observability_snapshot(&self) -> EngineConnectionObservabilitySnapshot {
        EngineConnectionObservabilitySnapshot {
            connected: self.connected.load(Ordering::Relaxed),
            last_disconnect_ms: {
                let ts = self.last_disconnect_ms.load(Ordering::Relaxed);
                (ts != 0).then_some(ts)
            },
            request_queue_capacity: self.request_queue_capacity,
            request_queue_depth: self.metrics.request_queue_depth.load(Ordering::Relaxed),
            request_queue_high_water: self.metrics.request_queue_high_water.load(Ordering::Relaxed),
            pending_requests: self.metrics.pending_requests.load(Ordering::Relaxed),
            pending_requests_high_water: self.metrics.pending_requests_high_water.load(Ordering::Relaxed),
            connect_count: self.metrics.connect_count.load(Ordering::Relaxed),
            disconnect_count: self.metrics.disconnect_count.load(Ordering::Relaxed),
            request_send_timeouts: self.metrics.request_send_timeouts.load(Ordering::Relaxed),
            request_send_failures: self.metrics.request_send_failures.load(Ordering::Relaxed),
            request_timeouts: self.metrics.request_timeouts.load(Ordering::Relaxed),
            disconnected_pending_requests: self.metrics.disconnected_pending_requests.load(Ordering::Relaxed),
            completed_roundtrips: self.metrics.completed_roundtrips.load(Ordering::Relaxed),
            roundtrip_total_ms: self.metrics.roundtrip_total_ms.load(Ordering::Relaxed),
            roundtrip_max_ms: self.metrics.roundtrip_max_ms.load(Ordering::Relaxed),
            unsolicited_events: self.metrics.unsolicited_events.load(Ordering::Relaxed),
            stale_response_drops: self.metrics.stale_response_drops.load(Ordering::Relaxed),
            no_subscriber_event_drops: self.metrics.no_subscriber_event_drops.load(Ordering::Relaxed),
            event_subscribers: self.events.receiver_count() as u64,
            connect_event_subscribers: self.connect_events.receiver_count() as u64,
            active_streams: self.active_streams.load(Ordering::Relaxed),
            timeout_scale_ppm: self.timeout_scale_ppm.load(Ordering::Relaxed),
        }
    }
}

struct EngineRequest {
    command_id: CommandId,
    command: EngineCommand,
    expected: ExpectedEvent,
    respond_to: oneshot::Sender<Result<EngineEvent, lib_ipc::client::ClientTransportError>>,
    label: &'static str,
    issued_at: Instant,
    deadline: Instant,
    saw_transport_ack: bool,
    journal_mode: JournalMode,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum JournalMode {
    Durable,
    Ephemeral,
}

impl JournalMode {
    fn for_command(command: &EngineCommand) -> Self {
        match command {
            EngineCommand::SolveLocalization { .. }
            | EngineCommand::GetLocalizationPipelineStatus { .. }
            | EngineCommand::ListLocalizationPipelineOutputs { .. }
            | EngineCommand::SampleLocalizationPipelineOutput { .. } => Self::Ephemeral,
            _ => Self::Durable,
        }
    }
}

#[derive(Copy, Clone, Debug)]
enum ExpectedEvent {
    Start,
    Stop { stream_id: uuid::Uuid },
    Controls { stream_id: uuid::Uuid },
    Metrics { stream_id: uuid::Uuid },
    SnapshotJpeg { stream_id: uuid::Uuid },
    GraphOutputs { stream_id: uuid::Uuid },
    GraphOutputSample { stream_id: uuid::Uuid },
    Ack,
    StreamList,
    StreamRuntimeCapabilities,
    NodeRegistry,
    Discovery,
    GraphValidation,
    CalibrationSolved,
    LocalizationSolved,
    LocalizationPipelineStatus,
    LocalizationPipelineOutputs,
    LocalizationPipelineOutputSample,
}

impl ExpectedEvent {
    fn matches(&self, event: &EngineEvent) -> bool {
        match self {
            ExpectedEvent::Start => matches!(event, EngineEvent::Started { .. } | EngineEvent::Nack { .. }),
            ExpectedEvent::Stop { stream_id } => matches!(event, EngineEvent::Stopped { stream_id: sid, .. } if sid == stream_id) || matches!(event, EngineEvent::Nack { .. }),
            ExpectedEvent::Controls { stream_id } => matches!(event, EngineEvent::Controls { stream_id: sid, .. } if sid == stream_id) || matches!(event, EngineEvent::Nack { .. }),
            ExpectedEvent::Metrics { stream_id } => matches!(event, EngineEvent::Metrics { stream_id: sid, .. } if sid == stream_id) || matches!(event, EngineEvent::Nack { .. }),
            ExpectedEvent::SnapshotJpeg { stream_id } => matches!(event, EngineEvent::SnapshotJpeg { stream_id: sid, .. } if sid == stream_id) || matches!(event, EngineEvent::Nack { .. }),
            ExpectedEvent::GraphOutputs { stream_id } => matches!(event, EngineEvent::GraphOutputs { stream_id: sid, .. } if sid == stream_id) || matches!(event, EngineEvent::Nack { .. }),
            ExpectedEvent::GraphOutputSample { stream_id } => matches!(event, EngineEvent::GraphOutputSample { stream_id: sid, .. } if sid == stream_id) || matches!(event, EngineEvent::Nack { .. }),
            ExpectedEvent::Ack => matches!(event, EngineEvent::Ack { .. } | EngineEvent::Nack { .. }),
            ExpectedEvent::StreamList => matches!(event, EngineEvent::StreamList { .. }),
            ExpectedEvent::StreamRuntimeCapabilities => matches!(event, EngineEvent::StreamRuntimeCapabilities { .. } | EngineEvent::Nack { .. }),
            ExpectedEvent::NodeRegistry => {
                matches!(event, EngineEvent::NodeRegistry { .. } | EngineEvent::Nack { .. })
            }
            ExpectedEvent::Discovery => {
                matches!(event, EngineEvent::Discovery { .. } | EngineEvent::Nack { .. })
            }
            ExpectedEvent::GraphValidation => {
                matches!(event, EngineEvent::GraphValidation { .. } | EngineEvent::Nack { .. })
            }
            ExpectedEvent::CalibrationSolved => {
                matches!(event, EngineEvent::CalibrationSolved { .. } | EngineEvent::Nack { .. })
            }
            ExpectedEvent::LocalizationSolved => {
                matches!(event, EngineEvent::LocalizationSolved { .. } | EngineEvent::Nack { .. })
            }
            ExpectedEvent::LocalizationPipelineStatus => {
                matches!(event, EngineEvent::LocalizationPipelineStatus { .. } | EngineEvent::Nack { .. })
            }
            ExpectedEvent::LocalizationPipelineOutputs => {
                matches!(event, EngineEvent::LocalizationPipelineOutputs { .. } | EngineEvent::Nack { .. })
            }
            ExpectedEvent::LocalizationPipelineOutputSample => {
                matches!(event, EngineEvent::LocalizationPipelineOutputSample { .. } | EngineEvent::Nack { .. })
            }
        }
    }
}

fn disconnected_error() -> lib_ipc::client::ClientTransportError {
    lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "engine connection unavailable"))
}

fn mark_disconnected(connected: &AtomicBool, last_disconnect_ms: &AtomicU64) {
    connected.store(false, Ordering::Relaxed);
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    last_disconnect_ms.store(ts, Ordering::Relaxed);
}

fn update_high_water(counter: &AtomicUsize, value: usize) {
    let mut current = counter.load(Ordering::Relaxed);
    while value > current {
        match counter.compare_exchange(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}
