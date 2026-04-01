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
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{Duration, Instant};

use super::command_id_from_context;
use timeouts::{engine_request_send_timeout, scale_timeout, timeout_scale_for_streams};
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

#[derive(Debug)]
pub struct EngineConnection {
    requests: mpsc::Sender<EngineRequest>,
    /// Fan-out for unsolicited engine events so callers can observe telemetry without blocking RPCs.
    events: broadcast::Sender<EngineEvent>,
    connect_events: broadcast::Sender<()>,
    connected: Arc<AtomicBool>,
    last_disconnect_ms: Arc<AtomicU64>,
    timeout_scale_ppm: Arc<AtomicU64>,
    active_streams: Arc<AtomicUsize>,
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
        let msg = EngineRequest { command_id, command, expected, respond_to: tx, label, deadline: Instant::now() + timeout, saw_transport_ack: false, journal_mode };
        let send_timeout = self.scaled_timeout(engine_request_send_timeout());
        match tokio::time::timeout(send_timeout, self.requests.send(msg)).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                return Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "engine dispatcher stopped")));
            }
            Err(_) => {
                return Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::TimedOut, "engine request queue is full")));
            }
        }
        rx.await.unwrap_or_else(|_| Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::BrokenPipe, "engine dispatcher dropped response"))))
    }
}

struct EngineRequest {
    command_id: CommandId,
    command: EngineCommand,
    expected: ExpectedEvent,
    respond_to: oneshot::Sender<Result<EngineEvent, lib_ipc::client::ClientTransportError>>,
    label: &'static str,
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
