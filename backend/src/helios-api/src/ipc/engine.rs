use std::{
    future, io,
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use helios_engine::ipc::{CalibrationSolveRequest, EngineCommand, EngineEvent, JsonWire, NodeRegistrySnapshot, StreamCalibration, StreamManifest};
use lib_ipc::client::{Client as GenericClient, Session as GenericSession, TransportConfig};
use lib_ipc::types::CommandId;
use lib_ipc::types::FeatureSet;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::{Duration, Instant, sleep, timeout};
use tracing::{error, info, warn};

use super::{JOURNAL_DIR, command_id_from_context};

const ENGINE_SOCKET: &str = "/run/helios/engine.sock";
const DEV_ENGINE_SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/dev/run/engine.sock");
// Engine responses are delivered over a broadcast event stream; under load it's possible to
// occasionally miss or delay the specific response event even though the command was accepted.
// Keep this conservative to avoid spurious UI failures requiring repeated clicks.
const ENGINE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
const ENGINE_PIPELINE_LAYOUT_TIMEOUT: Duration = Duration::from_secs(30);
const ENGINE_START_STREAM_TIMEOUT: Duration = Duration::from_secs(60);
const ENGINE_STOP_STREAM_TIMEOUT: Duration = Duration::from_secs(30);
// Recording finalization (ffmpeg remux/transcode) and shadow-capture can take far longer than
// "normal" engine RPCs, especially on slower devices and for large windows.
const ENGINE_STOP_RECORDING_TIMEOUT: Duration = Duration::from_secs(180);
const ENGINE_CAPTURE_SHADOW_TIMEOUT: Duration = Duration::from_secs(300);
fn calibration_solve_timeout() -> Duration {
    const DEFAULT_SECS: u64 = 300;
    const MIN_SECS: u64 = 30;
    const MAX_SECS: u64 = 1800;
    let secs = std::env::var("HELIOS_CALIBRATION_SOLVE_TIMEOUT_SECS").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_SECS).clamp(MIN_SECS, MAX_SECS);
    Duration::from_secs(secs)
}
const ENGINE_RECONNECT_INITIAL: Duration = Duration::from_millis(200);
const ENGINE_RECONNECT_MAX: Duration = Duration::from_secs(5);
const TIMEOUT_SCALE_PPM_BASE: u64 = 1_000_000;

fn read_timeout_env(var: &str, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let ms = std::env::var(var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);
    Duration::from_millis(ms.clamp(min_ms, max_ms))
}

fn read_usize_env(var: &str, default_value: usize, min_value: usize, max_value: usize) -> usize {
    let value = std::env::var(var).ok().and_then(|raw| raw.trim().parse::<usize>().ok()).unwrap_or(default_value);
    value.clamp(min_value, max_value)
}

fn engine_request_send_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_ENGINE_REQUEST_SEND_TIMEOUT_MS", 1_000, 50, 10_000))
}

fn engine_command_send_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_ENGINE_COMMAND_SEND_TIMEOUT_MS", 2_000, 100, 15_000))
}

fn engine_request_queue_size(stream_count: usize) -> usize {
    let base = read_usize_env("HELIOS_ENGINE_REQUEST_QUEUE", 256, 32, 8_192);
    let per_stream = read_usize_env("HELIOS_ENGINE_REQUEST_QUEUE_PER_STREAM", 8, 0, 256);
    let max = read_usize_env("HELIOS_ENGINE_REQUEST_QUEUE_MAX", 2_048, base, 8_192);
    let extra = stream_count.saturating_mul(per_stream);
    base.saturating_add(extra).min(max)
}

fn timeout_scale_per_stream_ppm() -> u64 {
    static VALUE: OnceLock<u64> = OnceLock::new();
    *VALUE.get_or_init(|| std::env::var("HELIOS_ENGINE_TIMEOUT_PER_STREAM_PPM").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(50_000))
}

fn timeout_scale_max_ppm() -> u64 {
    static VALUE: OnceLock<u64> = OnceLock::new();
    *VALUE.get_or_init(|| std::env::var("HELIOS_ENGINE_TIMEOUT_SCALE_MAX_PPM").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(2_000_000))
}

fn timeout_scale_for_streams(stream_count: usize) -> u64 {
    let per_stream = timeout_scale_per_stream_ppm();
    let max = timeout_scale_max_ppm().max(TIMEOUT_SCALE_PPM_BASE);
    let scale = TIMEOUT_SCALE_PPM_BASE.saturating_add(per_stream.saturating_mul(stream_count as u64));
    scale.min(max)
}

fn scale_timeout(base: Duration, scale_ppm: u64) -> Duration {
    if base.is_zero() || scale_ppm == TIMEOUT_SCALE_PPM_BASE {
        return base;
    }
    let base_ms = base.as_millis().min(u128::from(u64::MAX)) as u64;
    let scaled = base_ms.saturating_mul(scale_ppm).saturating_div(TIMEOUT_SCALE_PPM_BASE);
    Duration::from_millis(scaled.max(1))
}

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
        let (tx, rx) = oneshot::channel();
        let msg = EngineRequest { command_id, command, expected, respond_to: tx, label, deadline: Instant::now() + timeout, saw_transport_ack: false };
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

    pub async fn start_stream(&self, manifest: StreamManifest) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::Start { command_id, manifest: Box::new(manifest) }, ExpectedEvent::Start, "start_stream", ENGINE_START_STREAM_TIMEOUT).await
    }

    pub async fn stop_stream(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.stop_stream_with_timeout(id, ENGINE_STOP_STREAM_TIMEOUT).await
    }

    pub async fn stop_stream_with_timeout(&self, id: uuid::Uuid, timeout: Duration) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::Stop { command_id, stream_id: id }, ExpectedEvent::Stop { stream_id: id }, "stop_stream", timeout).await
    }

    pub async fn get_controls(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::GetControls { command_id, stream_id: id }, ExpectedEvent::Controls { stream_id: id }, "get_controls", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn get_metrics(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::GetMetrics { command_id, stream_id: id }, ExpectedEvent::Metrics { stream_id: id }, "get_metrics", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn snapshot_jpeg(&self, id: uuid::Uuid, quality: u8, source: Option<helios_engine::ipc::RecordingSource>) -> Result<Vec<u8>, lib_ipc::client::ClientTransportError> {
        match self
            .request(|command_id| EngineCommand::SnapshotJpeg { command_id, stream_id: id, quality, source }, ExpectedEvent::SnapshotJpeg { stream_id: id }, "snapshot_jpeg", ENGINE_RESPONSE_TIMEOUT)
            .await?
        {
            EngineEvent::SnapshotJpeg { bytes, .. } => Ok(bytes),
            EngineEvent::Nack { reason, .. } => Err(lib_ipc::client::ClientTransportError::Io(io::Error::other(reason))),
            other => {
                warn!(?other, "engine returned unexpected event for snapshot_jpeg after filtering");
                Err(lib_ipc::client::ClientTransportError::UnexpectedMessage { expected: lib_ipc::frame::MessageKind::Event, received: lib_ipc::frame::MessageKind::Event })
            }
        }
    }

    pub async fn start_recording(&self, id: uuid::Uuid, params: StartRecordingParams) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        let StartRecordingParams { source, output_path, container, codec, duration_ms, settings } = params;
        self.request(
            |command_id| EngineCommand::StartRecording { command_id, stream_id: id, source, output_path, container, codec, duration_ms, settings },
            ExpectedEvent::Ack,
            "start_recording",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn stop_recording(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::StopRecording { command_id, stream_id: id }, ExpectedEvent::Ack, "stop_recording", ENGINE_STOP_RECORDING_TIMEOUT).await
    }

    pub async fn capture_shadow_recording(
        &self,
        id: uuid::Uuid,
        output_path: String,
        container: helios_engine::ipc::RecordingContainer,
        window_ms: u64,
    ) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(
            |command_id| EngineCommand::CaptureShadowRecording { command_id, stream_id: id, output_path, container, window_ms },
            ExpectedEvent::Ack,
            "capture_shadow_recording",
            ENGINE_CAPTURE_SHADOW_TIMEOUT,
        )
        .await
    }

    pub async fn get_node_registry(&self) -> Result<NodeRegistrySnapshot, lib_ipc::client::ClientTransportError> {
        self.get_node_registry_with_timeout(ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn get_node_registry_with_timeout(&self, timeout: Duration) -> Result<NodeRegistrySnapshot, lib_ipc::client::ClientTransportError> {
        match self.request(|command_id| EngineCommand::GetNodeRegistry { command_id }, ExpectedEvent::NodeRegistry, "get_node_registry", timeout).await? {
            EngineEvent::NodeRegistry { snapshot, .. } => Ok(snapshot),
            EngineEvent::Nack { reason, .. } => Err(lib_ipc::client::ClientTransportError::Io(io::Error::other(reason))),
            other => {
                warn!(?other, "engine returned unexpected event for get_node_registry after filtering");
                Err(lib_ipc::client::ClientTransportError::UnexpectedMessage { expected: lib_ipc::frame::MessageKind::Event, received: lib_ipc::frame::MessageKind::Event })
            }
        }
    }

    pub async fn refresh_node_registry(&self) -> Result<NodeRegistrySnapshot, lib_ipc::client::ClientTransportError> {
        match self.request(|command_id| EngineCommand::RefreshNodeRegistry { command_id }, ExpectedEvent::NodeRegistry, "refresh_node_registry", ENGINE_RESPONSE_TIMEOUT).await? {
            EngineEvent::NodeRegistry { snapshot, .. } => Ok(snapshot),
            EngineEvent::Nack { reason, .. } => Err(lib_ipc::client::ClientTransportError::Io(io::Error::other(reason))),
            other => {
                warn!(?other, "engine returned unexpected event for refresh_node_registry after filtering");
                Err(lib_ipc::client::ClientTransportError::UnexpectedMessage { expected: lib_ipc::frame::MessageKind::Event, received: lib_ipc::frame::MessageKind::Event })
            }
        }
    }

    pub async fn validate_graph_event(&self, graph: serde_json::Value, active_features: Vec<String>, enable_lints: bool) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(
            |command_id| EngineCommand::ValidateGraph { command_id, graph: graph.into(), active_features, enable_lints },
            ExpectedEvent::GraphValidation,
            "validate_graph",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn set_graph(&self, id: uuid::Uuid, graph: serde_json::Value, pipeline_id: Option<uuid::Uuid>, output: Option<String>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetGraph { command_id, stream_id: id, graph: graph.into(), pipeline_id, output }, ExpectedEvent::Ack, "set_graph", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_graph_patch(&self, id: uuid::Uuid, patch: serde_json::Value, pipeline_id: Option<uuid::Uuid>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetGraphPatch { command_id, stream_id: id, patch: patch.into(), pipeline_id }, ExpectedEvent::Ack, "set_graph_patch", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_control(&self, id: uuid::Uuid, control_id: u32, value: helios_engine::capture::CaptureControlValue) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetControl { command_id, stream_id: id, control_id, value }, ExpectedEvent::Ack, "set_control", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_graph_output(&self, id: uuid::Uuid, output: Option<String>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetGraphOutput { command_id, stream_id: id, output }, ExpectedEvent::Ack, "set_graph_output", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_pipeline_inputs(
        &self,
        id: uuid::Uuid,
        pipeline_id: Option<uuid::Uuid>,
        inputs: std::collections::BTreeMap<String, Option<serde_json::Value>>,
    ) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        let mapped = inputs.into_iter().map(|(key, value)| (key, value.map(JsonWire::from))).collect();
        self.request(|command_id| EngineCommand::SetPipelineInputs { command_id, stream_id: id, pipeline_id, inputs: mapped }, ExpectedEvent::Ack, "set_pipeline_inputs", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_pipeline_layout(&self, id: uuid::Uuid, layout: Option<helios_engine::ipc::StreamPipelineLayout>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetPipelineLayout { command_id, stream_id: id, layout }, ExpectedEvent::Ack, "set_pipeline_layout", ENGINE_PIPELINE_LAYOUT_TIMEOUT).await
    }

    pub async fn set_pipeline_wires(&self, id: uuid::Uuid, wires: Vec<helios_engine::ipc::StreamPipelineWire>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        // Wiring changes may trigger a full multiplex rebuild, so keep this slightly more lenient
        // than default RPC timeouts.
        self.request(|command_id| EngineCommand::SetPipelineWires { command_id, stream_id: id, wires }, ExpectedEvent::Ack, "set_pipeline_wires", ENGINE_PIPELINE_LAYOUT_TIMEOUT).await
    }

    pub async fn set_graph_perf(&self, id: uuid::Uuid, pipeline_id: Option<uuid::Uuid>, enabled: bool) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetGraphPerf { command_id, stream_id: id, pipeline_id, enabled }, ExpectedEvent::Ack, "set_graph_perf", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn reset_graph_metrics(&self, id: uuid::Uuid, pipeline_id: Option<uuid::Uuid>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::ResetGraphMetrics { command_id, stream_id: id, pipeline_id }, ExpectedEvent::Ack, "reset_graph_metrics", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn capture_graph_flamegraph(&self, id: uuid::Uuid, pipeline_id: Option<uuid::Uuid>, duration_ms: u64) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(
            |command_id| EngineCommand::CaptureGraphFlamegraph { command_id, stream_id: id, pipeline_id, duration_ms },
            ExpectedEvent::Ack,
            "capture_graph_flamegraph",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn list_graph_outputs_event(&self, id: uuid::Uuid) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::ListGraphOutputs { command_id, stream_id: id }, ExpectedEvent::GraphOutputs { stream_id: id }, "list_graph_outputs", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn get_graph_output_sample_event(&self, id: uuid::Uuid, port: String) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(
            |command_id| EngineCommand::GetGraphOutputSample { command_id, stream_id: id, port: port.clone() },
            ExpectedEvent::GraphOutputSample { stream_id: id },
            "get_graph_output_sample",
            ENGINE_RESPONSE_TIMEOUT,
        )
        .await
    }

    pub async fn set_codecs(&self, id: uuid::Uuid, decoder_id: Option<String>, encoder_id: Option<String>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetCodecs { command_id, stream_id: id, decoder_id, encoder_id }, ExpectedEvent::Ack, "set_codecs", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_calibration(&self, id: uuid::Uuid, calibration: Option<StreamCalibration>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetCalibration { command_id, stream_id: id, calibration }, ExpectedEvent::Ack, "set_calibration", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn set_calibration_mode(&self, id: uuid::Uuid, enabled: bool, dictionary: Option<String>, mode: Option<String>) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SetCalibrationMode { command_id, stream_id: id, enabled, dictionary, mode }, ExpectedEvent::Ack, "set_calibration_mode", ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn solve_calibration_event(&self, request: CalibrationSolveRequest) -> Result<EngineEvent, lib_ipc::client::ClientTransportError> {
        self.request(|command_id| EngineCommand::SolveCalibration { command_id, request }, ExpectedEvent::CalibrationSolved, "solve_calibration", calibration_solve_timeout()).await
    }

    pub async fn list_streams(&self) -> Result<Vec<helios_engine::ipc::StreamSummary>, lib_ipc::client::ClientTransportError> {
        self.list_streams_with_timeout(ENGINE_RESPONSE_TIMEOUT).await
    }

    pub async fn list_streams_with_timeout(&self, timeout: Duration) -> Result<Vec<helios_engine::ipc::StreamSummary>, lib_ipc::client::ClientTransportError> {
        match self.request(|command_id| EngineCommand::List { command_id }, ExpectedEvent::StreamList, "list_streams", timeout).await? {
            EngineEvent::StreamList { streams, .. } => {
                self.update_timeout_scale_from_count(streams.len());
                Ok(streams)
            }
            other => {
                warn!(?other, "engine returned unexpected event for list_streams after filtering");
                Err(lib_ipc::client::ClientTransportError::UnexpectedMessage { expected: lib_ipc::frame::MessageKind::Event, received: lib_ipc::frame::MessageKind::Event })
            }
        }
    }

    /// Subscribe to unsolicited engine events (those not tied to a specific in-flight request).
    pub fn subscribe_events(&self) -> broadcast::Receiver<EngineEvent> {
        self.events.subscribe()
    }

    pub fn subscribe_connect_events(&self) -> broadcast::Receiver<()> {
        self.connect_events.subscribe()
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn last_disconnect_ms(&self) -> Option<u64> {
        let ts = self.last_disconnect_ms.load(Ordering::Relaxed);
        if ts == 0 { None } else { Some(ts) }
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
    NodeRegistry,
    GraphValidation,
    CalibrationSolved,
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
            ExpectedEvent::NodeRegistry => matches!(event, EngineEvent::NodeRegistry { .. } | EngineEvent::Nack { .. }),
            ExpectedEvent::GraphValidation => matches!(event, EngineEvent::GraphValidation { .. } | EngineEvent::Nack { .. }),
            ExpectedEvent::CalibrationSolved => matches!(event, EngineEvent::CalibrationSolved { .. } | EngineEvent::Nack { .. }),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_engine_dispatcher(
    client: Arc<EngineClient>,
    mut rx: mpsc::Receiver<EngineRequest>,
    events: broadcast::Sender<EngineEvent>,
    connect_events: broadcast::Sender<()>,
    connected: Arc<AtomicBool>,
    last_disconnect_ms: Arc<AtomicU64>,
    timeout_scale_ppm: Arc<AtomicU64>,
    active_streams: Arc<AtomicUsize>,
) {
    use std::collections::HashMap;

    let mut pending: HashMap<CommandId, EngineRequest> = HashMap::new();
    let mut session: Option<EngineSession> = None;
    let mut backoff = ENGINE_RECONNECT_INITIAL;
    let mut rx_closed = false;

    loop {
        if session.is_none() {
            match timeout(Duration::from_secs(5), client.handshake()).await {
                Ok(Ok(sess)) => {
                    info!("connected to engine IPC");
                    session = Some(sess);
                    backoff = ENGINE_RECONNECT_INITIAL;
                    let _ = connect_events.send(());
                    connected.store(true, Ordering::Relaxed);
                }
                Ok(Err(err)) => {
                    error!(%err, "engine handshake failed, will retry");
                    flush_pending_disconnect(&mut pending);
                    mark_disconnected(&connected, &last_disconnect_ms);
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(ENGINE_RECONNECT_MAX);
                    continue;
                }
                Err(_) => {
                    error!("engine handshake timed out, will retry");
                    flush_pending_disconnect(&mut pending);
                    mark_disconnected(&connected, &last_disconnect_ms);
                    sleep(backoff).await;
                    backoff = (backoff * 2).min(ENGINE_RECONNECT_MAX);
                    continue;
                }
            }
        }

        let next_deadline = pending.values().map(|req| req.deadline).min();
        let mut session_event = None;
        if let Some(sess) = session.as_mut() {
            session_event = Some(sess.next_event());
        }

        tokio::select! {
            biased;
            maybe_request = rx.recv() => {
                match maybe_request {
                    Some(request) => {
                        let mut disconnect = false;
                        if let Some(sess) = session.as_mut() {
                            let send_timeout = scale_timeout(engine_command_send_timeout(), timeout_scale_ppm.load(Ordering::Relaxed));
                            match tokio::time::timeout(send_timeout, sess.send_command(client.journal(), &request.command)).await {
                                Ok(Ok(_)) => {
                                    pending.insert(request.command_id, request);
                                }
                                Ok(Err(err)) => {
                                    let _ = request.respond_to.send(Err(err));
                                }
                                Err(_) => {
                                    let _ = request.respond_to.send(Err(lib_ipc::client::ClientTransportError::Io(
                                        io::Error::new(io::ErrorKind::TimedOut, "engine command send timed out"),
                                    )));
                                    disconnect = true;
                                }
                            }
                        } else {
                            let _ = request.respond_to.send(Err(disconnected_error()));
                        }
                        if disconnect {
                            flush_pending_disconnect(&mut pending);
                            session = None;
                            mark_disconnected(&connected, &last_disconnect_ms);
                        }
                    }
                    None => {
                        rx_closed = true;
                        if pending.is_empty() {
                            break;
                        }
                    }
                }
            }
            event_result = async {
                if let Some(fut) = session_event {
                    fut.await
                } else {
                    Ok(None)
                }
            } => {
                let event = match event_result {
                    Ok(Some(ev)) => ev,
                    Ok(None) => {
                        flush_pending_disconnect(&mut pending);
                        session = None;
                        mark_disconnected(&connected, &last_disconnect_ms);
                        continue;
                    }
                    Err(err) => {
                        error!(%err, "engine session error; reconnecting");
                        flush_pending_disconnect(&mut pending);
                        session = None;
                        mark_disconnected(&connected, &last_disconnect_ms);
                        continue;
                    }
                };

                expire_timeouts(&mut pending);

                update_scale_from_event(&event, &timeout_scale_ppm, &active_streams);

                let mut delivered = false;
                if let Some(command_id) = event.command_id()
                    && let Some(mut req) = pending.remove(&command_id)
                {
                    // Engine IPC emits a transport-level ACK before the runtime publishes its
                    // typed command result. For commands waiting on Ack/Nack, treat the first
                    // Ack as transport-only and continue waiting for the runtime event.
                    if matches!(req.expected, ExpectedEvent::Ack) && matches!(event, EngineEvent::Ack { .. }) && !req.saw_transport_ack {
                        req.saw_transport_ack = true;
                        pending.insert(command_id, req);
                        delivered = true;
                    } else if req.expected.matches(&event) {
                        let _ = req.respond_to.send(Ok(event.clone()));
                        delivered = true;
                    } else {
                        // The engine IPC layer can emit an early transport-level Ack before the
                        // actual typed response event. Don't warn for that case; keep waiting.
                        if !matches!(event, EngineEvent::Ack { .. }) {
                            warn!(?event, expected = ?req.expected, "engine returned unexpected event; keeping request pending");
                        }
                        pending.insert(command_id, req);
                        delivered = true;
                    }
                }

                if !delivered && matches!(event, EngineEvent::CalibrationSolved { .. }) {
                    let mut candidates: Vec<CommandId> = pending
                        .iter()
                        .filter_map(|(id, req)| matches!(req.expected, ExpectedEvent::CalibrationSolved).then_some(*id))
                        .collect();
                    if candidates.len() == 1 {
                        let id = candidates.pop().unwrap();
                        if let Some(req) = pending.remove(&id) {
                            if req.expected.matches(&event) {
                                let _ = req.respond_to.send(Ok(event.clone()));
                                delivered = true;
                            } else {
                                pending.insert(id, req);
                            }
                        }
                    } else if candidates.len() > 1 {
                        warn!(count = candidates.len(), "multiple pending calibration requests; ignoring unmatched CalibrationSolved event");
                    }
                }

                if !delivered {
                    if events.receiver_count() > 0 {
                        let _ = events.send(event.clone());
                    } else if event.command_id().is_some() {
                        // Response arrived after we timed out; drop quietly.
                        tracing::debug!(?event, "dropping stale engine response");
                    } else if !matches!(event, EngineEvent::MetricsUpdate { .. }) {
                        warn!(?event, "received unsolicited engine event; no subscribers");
                    }
                }
            }
            _ = async {
                if let Some(deadline) = next_deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    future::pending::<()>().await;
                }
            } => {
                expire_timeouts(&mut pending);
            }
        }

        if rx_closed && pending.is_empty() {
            break;
        }
    }
}

fn update_scale_from_event(event: &EngineEvent, timeout_scale_ppm: &AtomicU64, active_streams: &AtomicUsize) {
    match event {
        EngineEvent::StreamList { streams, .. } => {
            let count = streams.len();
            active_streams.store(count, Ordering::Relaxed);
            timeout_scale_ppm.store(timeout_scale_for_streams(count), Ordering::Relaxed);
        }
        EngineEvent::Started { .. } => {
            update_scale_from_delta(timeout_scale_ppm, active_streams, 1);
        }
        EngineEvent::Stopped { .. } => {
            update_scale_from_delta(timeout_scale_ppm, active_streams, -1);
        }
        _ => {}
    }
}

fn update_scale_from_delta(timeout_scale_ppm: &AtomicU64, active_streams: &AtomicUsize, delta: i64) {
    let mut current = active_streams.load(Ordering::Relaxed);
    let delta_abs = if delta < 0 { (-delta) as usize } else { delta as usize };
    loop {
        let next = if delta < 0 { current.saturating_sub(delta_abs) } else { current.saturating_add(delta_abs) };
        match active_streams.compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => {
                timeout_scale_ppm.store(timeout_scale_for_streams(next), Ordering::Relaxed);
                break;
            }
            Err(observed) => current = observed,
        }
    }
}

fn expire_timeouts(pending: &mut std::collections::HashMap<CommandId, EngineRequest>) {
    let now = Instant::now();
    let expired: Vec<CommandId> = pending.iter().filter_map(|(id, req)| (req.deadline <= now).then_some(*id)).collect();
    for id in expired {
        if let Some(req) = pending.remove(&id) {
            let _ = req.respond_to.send(Err(lib_ipc::client::ClientTransportError::Io(io::Error::new(io::ErrorKind::TimedOut, format!("{} timed out waiting for engine event", req.label)))));
        }
    }
}

fn flush_pending_disconnect(pending: &mut std::collections::HashMap<CommandId, EngineRequest>) {
    let drained: Vec<EngineRequest> = pending.drain().map(|(_, req)| req).collect();
    for req in drained {
        let _ = req.respond_to.send(Err(disconnected_error()));
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

async fn fetch_stream_count_for_tuning(client: &EngineClient, mut session: EngineSession) -> Option<usize> {
    let command_id = CommandId::new();
    let command = EngineCommand::List { command_id };
    if session.send_command(client.journal(), &command).await.is_err() {
        return None;
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }
        let remaining = deadline - now;
        match timeout(remaining, session.next_event()).await {
            Ok(Ok(Some(event))) => match event {
                EngineEvent::StreamList { command_id: event_id, streams } if event_id == command_id => return Some(streams.len()),
                EngineEvent::Nack { command_id: event_id, .. } if event_id == command_id => return None,
                _ => continue,
            },
            Ok(Ok(None)) => return None,
            Ok(Err(_)) => return None,
            Err(_) => return None,
        }
    }
}

pub async fn connect_engine() -> Result<EngineConnection, Box<dyn std::error::Error + Send + Sync>> {
    let journal_path = PathBuf::from(JOURNAL_DIR).join("engine.journal");
    let candidates = resolve_engine_sockets();

    let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    for attempt in 0..5 {
        for socket in &candidates {
            match try_connect(socket, journal_path.clone()).await {
                Ok(conn) => return Ok(conn),
                Err(err) => {
                    if attempt == 4 {
                        last_err = Some(err);
                    }
                }
            }
        }
        sleep(Duration::from_millis(200)).await;
    }

    Err(last_err.unwrap_or_else(|| "engine IPC connect failed".into()))
}

pub async fn connect_engine_best_effort() -> EngineConnection {
    match connect_engine().await {
        Ok(conn) => return conn,
        Err(err) => warn!(%err, "engine IPC connect failed; starting in degraded mode"),
    }

    let journal_path = PathBuf::from(JOURNAL_DIR).join("engine.journal");
    let candidates = resolve_engine_sockets();
    for socket in &candidates {
        match try_connect_lazy(socket, journal_path.clone()) {
            Ok(conn) => return conn,
            Err(err) => warn!(%err, socket = %socket.display(), "engine IPC client init failed"),
        }
    }

    warn!("engine IPC unavailable; responding with disconnected errors until restart");
    spawn_unavailable_engine()
}

async fn try_connect(socket: &Path, journal_path: PathBuf) -> Result<EngineConnection, Box<dyn std::error::Error + Send + Sync>> {
    let config = EngineClientConfig::new(socket.to_path_buf(), journal_path);
    let client = Arc::new(EngineClient::new(config)?);
    // Perform an initial handshake to validate the endpoint and capture a rough stream count.
    let session = timeout(Duration::from_secs(5), client.handshake()).await??;
    let stream_count = fetch_stream_count_for_tuning(&client, session).await.unwrap_or(0);
    Ok(spawn_engine_connection(client, stream_count, true))
}

fn try_connect_lazy(socket: &Path, journal_path: PathBuf) -> Result<EngineConnection, Box<dyn std::error::Error + Send + Sync>> {
    let config = EngineClientConfig::new(socket.to_path_buf(), journal_path);
    let client = Arc::new(EngineClient::new(config)?);
    Ok(spawn_engine_connection(client, 0, false))
}

fn spawn_engine_connection(client: Arc<EngineClient>, stream_count: usize, connected_initial: bool) -> EngineConnection {
    let (tx, rx) = mpsc::channel(engine_request_queue_size(stream_count));
    let (events, _) = broadcast::channel(64);
    let (connect_events, _) = broadcast::channel(16);
    let connected = Arc::new(AtomicBool::new(connected_initial));
    let last_disconnect_ms = Arc::new(AtomicU64::new(0));
    let timeout_scale_ppm = Arc::new(AtomicU64::new(timeout_scale_for_streams(stream_count)));
    let active_streams = Arc::new(AtomicUsize::new(stream_count));
    if !connected_initial {
        mark_disconnected(&connected, &last_disconnect_ms);
    }
    tokio::spawn(run_engine_dispatcher(client.clone(), rx, events.clone(), connect_events.clone(), connected.clone(), last_disconnect_ms.clone(), timeout_scale_ppm.clone(), active_streams.clone()));
    EngineConnection { requests: tx, events, connect_events, connected, last_disconnect_ms, timeout_scale_ppm, active_streams }
}

fn spawn_unavailable_engine() -> EngineConnection {
    let (tx, mut rx) = mpsc::channel::<EngineRequest>(engine_request_queue_size(0));
    let (events, _) = broadcast::channel(64);
    let (connect_events, _) = broadcast::channel(16);
    let connected = Arc::new(AtomicBool::new(false));
    let last_disconnect_ms = Arc::new(AtomicU64::new(0));
    let timeout_scale_ppm = Arc::new(AtomicU64::new(timeout_scale_for_streams(0)));
    let active_streams = Arc::new(AtomicUsize::new(0));
    mark_disconnected(&connected, &last_disconnect_ms);
    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            let _ = req.respond_to.send(Err(disconnected_error()));
        }
    });
    EngineConnection { requests: tx, events, connect_events, connected, last_disconnect_ms, timeout_scale_ppm, active_streams }
}

fn resolve_engine_sockets() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(path) = std::env::var("HELIOS_ENGINE_SOCKET").or_else(|_| std::env::var("ENGINE_SOCKET")) {
        paths.push(PathBuf::from(path));
    }

    // Workspace dev path (backend/target/dev/run/engine.sock).
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    paths.push(manifest_dir.join("../..").join("target/dev/run/engine.sock"));

    // Crate-local dev target.
    paths.push(PathBuf::from(DEV_ENGINE_SOCKET));

    // System-installed socket.
    paths.push(PathBuf::from(ENGINE_SOCKET));

    // De-duplicate while preserving order.
    let mut deduped = Vec::new();
    for p in paths {
        if !deduped.iter().any(|seen: &PathBuf| seen == &p) {
            deduped.push(p);
        }
    }
    deduped
}
