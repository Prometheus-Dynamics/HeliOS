use super::*;
use crate::ipc::CalibrationSolveRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalizationSolveSourceValue {
    pub source_id: String,
    #[serde(default)]
    pub value: Option<JsonWire>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalizationSolveRequest {
    pub profile: crate::localization::config::LocalizationProfile,
    pub sources: Vec<crate::localization::config::LocalizationSourceConfig>,
    #[serde(default)]
    pub rig_poses: BTreeMap<String, crate::localization::types::LocalizationPose>,
    #[serde(default)]
    pub field_map: Option<crate::localization::maps::FieldMapDocument>,
    #[serde(default)]
    pub calibrations: BTreeMap<String, StreamCalibration>,
    #[serde(default)]
    pub source_values: Vec<LocalizationSolveSourceValue>,
    pub apply_field_origin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalizationPipelineStatusRequest {
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalizationPipelineGraphRequest {
    pub profile: crate::localization::config::LocalizationProfile,
    pub graph: JsonWire,
    #[serde(default)]
    pub graph_updated_at_ms: Option<i64>,
    #[serde(default)]
    pub template_mtime_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalizationPipelineSampleRequest {
    pub profile: crate::localization::config::LocalizationProfile,
    pub sources: Vec<crate::localization::config::LocalizationSourceConfig>,
    pub graph: JsonWire,
    #[serde(default)]
    pub graph_updated_at_ms: Option<i64>,
    #[serde(default)]
    pub template_mtime_ms: Option<i64>,
    #[serde(default)]
    pub source_values: Vec<LocalizationSolveSourceValue>,
    pub output_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum EngineCommand {
    List {
        command_id: CommandId,
    },
    GetStreamRuntimeCapabilities {
        command_id: CommandId,
    },
    Start {
        command_id: CommandId,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        manifest: Box<ResolvedStreamConfig>,
    },
    /// Update encoder/decoder selection for a running stream without restarting capture.
    SetCodecs {
        command_id: CommandId,
        stream_id: Uuid,
        decoder_id: Option<String>,
        encoder_id: Option<String>,
    },
    /// Update saved calibration for a running stream without restarting capture.
    SetCalibration {
        command_id: CommandId,
        stream_id: Uuid,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        calibration: Option<StreamCalibration>,
    },
    /// Toggle guided calibration mode (pass-through preview + live detections output) without restarting capture.
    SetCalibrationMode {
        command_id: CommandId,
        stream_id: Uuid,
        enabled: bool,
        dictionary: Option<String>,
        mode: Option<String>,
    },
    /// Solve camera intrinsics from calibration images + graph detections.
    SolveCalibration {
        command_id: CommandId,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        request: CalibrationSolveRequest,
    },
    SolveLocalization {
        command_id: CommandId,
        request: JsonWire,
    },
    GetLocalizationPipelineStatus {
        command_id: CommandId,
        request: JsonWire,
    },
    ListLocalizationPipelineOutputs {
        command_id: CommandId,
        request: JsonWire,
    },
    SampleLocalizationPipelineOutput {
        command_id: CommandId,
        request: JsonWire,
    },
    Stop {
        command_id: CommandId,
        stream_id: Uuid,
    },
    SetControl {
        command_id: CommandId,
        stream_id: Uuid,
        control_id: ControlId,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        value: CaptureControlValue,
    },
    GetControls {
        command_id: CommandId,
        stream_id: Uuid,
    },
    GetMetrics {
        command_id: CommandId,
        stream_id: Uuid,
    },
    /// Request a one-off JPEG snapshot from the latest decoded/graph-processed frame.
    ///
    /// This is intended for "take snapshot" flows where quality should be higher than the
    /// stream's live preview/output encoding settings.
    SnapshotJpeg {
        command_id: CommandId,
        stream_id: Uuid,
        /// JPEG quality in range 1..=100 (clamped server-side).
        quality: u8,
        /// Optional source selector. When omitted, snapshot uses the current preview source.
        #[serde(default)]
        source: Option<RecordingSource>,
    },
    GetNodeRegistry {
        command_id: CommandId,
    },
    DiscoverDevices {
        command_id: CommandId,
    },
    RefreshNodeRegistry {
        command_id: CommandId,
    },
    ValidateGraph {
        command_id: CommandId,
        graph: JsonWire,
        active_features: Vec<String>,
        enable_lints: bool,
    },
    /// Update the active pipeline graph for a running stream without restarting capture.
    SetGraph {
        command_id: CommandId,
        stream_id: Uuid,
        graph: JsonWire,
        pipeline_id: Option<Uuid>,
        output: Option<String>,
    },
    /// Apply a graph patch (node constant overrides) to a running stream without rebuilding.
    SetGraphPatch {
        command_id: CommandId,
        stream_id: Uuid,
        patch: JsonWire,
        pipeline_id: Option<Uuid>,
    },
    /// Update which graph output feeds the stream preview/encoder without restarting the stream.
    SetGraphOutput {
        command_id: CommandId,
        stream_id: Uuid,
        output: Option<String>,
    },
    /// Update pipeline input values for a running stream without rebuilding the graph.
    SetPipelineInputs {
        command_id: CommandId,
        stream_id: Uuid,
        pipeline_id: Option<Uuid>,
        inputs: BTreeMap<String, Option<JsonWire>>,
    },
    /// List host-bridge output ports (graph -> host) exposed by the running stream graph.
    ListGraphOutputs {
        command_id: CommandId,
        stream_id: Uuid,
    },
    /// List host-bridge output ports plus solved typing info for the running stream graph.
    // (folded into ListGraphOutputs)
    /// Fetch the latest JSON payload captured from a graph output port.
    GetGraphOutputSample {
        command_id: CommandId,
        stream_id: Uuid,
        port: String,
        fresh: bool,
    },
    /// Update multiplex layout (rows/columns/slot assignment) without restarting capture.
    SetPipelineLayout {
        command_id: CommandId,
        stream_id: Uuid,
        layout: Option<StreamPipelineLayout>,
    },
    /// Update wiring between pipeline outputs and downstream pipeline inputs.
    SetPipelineWires {
        command_id: CommandId,
        stream_id: Uuid,
        wires: Vec<StreamPipelineWire>,
    },
    /// Enable/disable perf counters collection for the running graph.
    SetGraphPerf {
        command_id: CommandId,
        stream_id: Uuid,
        pipeline_id: Option<Uuid>,
        enabled: bool,
    },
    /// Reset rolling pipeline metrics (node timings/perf samples/flamegraph).
    ResetGraphMetrics {
        command_id: CommandId,
        stream_id: Uuid,
        pipeline_id: Option<Uuid>,
    },
    /// Capture a CPU flamegraph over wall-clock time and store it on disk.
    CaptureGraphFlamegraph {
        command_id: CommandId,
        stream_id: Uuid,
        pipeline_id: Option<Uuid>,
        duration_ms: u64,
    },
    /// Start recording a stream to a media file.
    StartRecording {
        command_id: CommandId,
        stream_id: Uuid,
        source: RecordingSource,
        output_path: String,
        container: RecordingContainer,
        codec: RecordingCodec,
        duration_ms: Option<u64>,
        settings: Option<RecordingSettings>,
    },
    /// Stop an active recording for a stream.
    StopRecording {
        command_id: CommandId,
        stream_id: Uuid,
    },
    /// Capture the last N milliseconds from the shadow recorder buffer.
    CaptureShadowRecording {
        command_id: CommandId,
        stream_id: Uuid,
        output_path: String,
        container: RecordingContainer,
        window_ms: u64,
    },
}

impl EngineCommand {
    pub fn command_id(&self) -> Option<CommandId> {
        Some(match self {
            EngineCommand::List { command_id }
            | EngineCommand::GetStreamRuntimeCapabilities { command_id }
            | EngineCommand::Start { command_id, .. }
            | EngineCommand::SetCodecs { command_id, .. }
            | EngineCommand::SetCalibration { command_id, .. }
            | EngineCommand::SetCalibrationMode { command_id, .. }
            | EngineCommand::SolveCalibration { command_id, .. }
            | EngineCommand::SolveLocalization { command_id, .. }
            | EngineCommand::GetLocalizationPipelineStatus { command_id, .. }
            | EngineCommand::ListLocalizationPipelineOutputs { command_id, .. }
            | EngineCommand::SampleLocalizationPipelineOutput { command_id, .. }
            | EngineCommand::Stop { command_id, .. }
            | EngineCommand::SetControl { command_id, .. }
            | EngineCommand::GetControls { command_id, .. }
            | EngineCommand::GetMetrics { command_id, .. }
            | EngineCommand::SnapshotJpeg { command_id, .. }
            | EngineCommand::GetNodeRegistry { command_id }
            | EngineCommand::DiscoverDevices { command_id }
            | EngineCommand::RefreshNodeRegistry { command_id }
            | EngineCommand::ValidateGraph { command_id, .. }
            | EngineCommand::SetGraph { command_id, .. }
            | EngineCommand::SetGraphPatch { command_id, .. }
            | EngineCommand::SetGraphOutput { command_id, .. }
            | EngineCommand::SetPipelineInputs { command_id, .. }
            | EngineCommand::ListGraphOutputs { command_id, .. }
            | EngineCommand::GetGraphOutputSample { command_id, .. }
            | EngineCommand::SetPipelineLayout { command_id, .. }
            | EngineCommand::SetPipelineWires { command_id, .. }
            | EngineCommand::SetGraphPerf { command_id, .. }
            | EngineCommand::ResetGraphMetrics { command_id, .. }
            | EngineCommand::CaptureGraphFlamegraph { command_id, .. }
            | EngineCommand::StartRecording { command_id, .. }
            | EngineCommand::StopRecording { command_id, .. }
            | EngineCommand::CaptureShadowRecording { command_id, .. } => *command_id,
        })
    }
}

impl RequestIdentity for EngineCommand {
    fn request_id(&self) -> CommandId {
        self.command_id().expect("engine command must carry a request id")
    }
}

impl EngineEvent {
    pub fn command_id(&self) -> Option<CommandId> {
        match self {
            EngineEvent::Ack { command_id, .. }
            | EngineEvent::Nack { command_id, .. }
            | EngineEvent::StreamList { command_id, .. }
            | EngineEvent::StreamRuntimeCapabilities { command_id, .. }
            | EngineEvent::Started { command_id, .. }
            | EngineEvent::Stopped { command_id, .. }
            | EngineEvent::Controls { command_id, .. }
            | EngineEvent::Metrics { command_id, .. }
            | EngineEvent::SnapshotJpeg { command_id, .. }
            | EngineEvent::GraphOutputs { command_id, .. }
            | EngineEvent::GraphOutputSample { command_id, .. }
            | EngineEvent::NodeRegistry { command_id, .. }
            | EngineEvent::Discovery { command_id, .. }
            | EngineEvent::GraphValidation { command_id, .. }
            | EngineEvent::CalibrationSolved { command_id, .. }
            | EngineEvent::LocalizationSolved { command_id, .. }
            | EngineEvent::LocalizationPipelineStatus { command_id, .. }
            | EngineEvent::LocalizationPipelineOutputs { command_id, .. }
            | EngineEvent::LocalizationPipelineOutputSample { command_id, .. } => Some(*command_id),
            EngineEvent::MetricsUpdate { .. } => None,
        }
    }
}
