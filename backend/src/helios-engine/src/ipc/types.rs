use lib_cv::modules::calibration::LensModel;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::env;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::capture::{CaptureConfig, CaptureControlInfo, CaptureControlValue, CaptureDescriptor};
use crate::identity::DeviceIdentity;
use crate::stream::StreamMetrics;

use bincode::error::{DecodeError, EncodeError};
use bincode::{Decode, Encode};
use lib_ipc::frame::MessageKind;
use lib_ipc::protocol::ControlEvent;
use lib_ipc::server::ServerEvent;
use lib_ipc::types::CommandId;

pub type ControlId = u32;

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, ToSchema, PartialEq)]
pub struct GraphOutputPortDescriptor {
    pub name: String,
    #[serde(default)]
    #[bincode(with_serde)]
    pub ty: Option<JsonWire>,
    pub previewable: bool,
}

/// A serde JSON value that remains JSON in HTTP/OpenAPI payloads, but is encoded as JSON bytes when
/// serialized over binary transports (e.g. bincode over IPC).
///
/// This avoids `bincode::serde` limitations around `deserialize_any` while keeping the public JSON
/// shape unchanged.
#[derive(Debug, Clone, PartialEq, ToSchema)]
#[schema(value_type = serde_json::Value)]
pub struct JsonWire(pub JsonValue);

impl JsonWire {
    pub fn as_value(&self) -> &JsonValue {
        &self.0
    }
}

impl From<JsonValue> for JsonWire {
    fn from(value: JsonValue) -> Self {
        Self(value)
    }
}

impl From<JsonWire> for JsonValue {
    fn from(value: JsonWire) -> Self {
        value.0
    }
}

impl Serialize for JsonWire {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.0.serialize(serializer)
        } else {
            let bytes = serde_json::to_vec(&self.0).map_err(serde::ser::Error::custom)?;
            serde_bytes::serialize(&bytes, serializer)
        }
    }
}

impl<'de> Deserialize<'de> for JsonWire {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            Ok(Self(JsonValue::deserialize(deserializer)?))
        } else {
            let bytes: Vec<u8> = serde_bytes::deserialize(deserializer)?;
            let value = serde_json::from_slice(&bytes).map_err(serde::de::Error::custom)?;
            Ok(Self(value))
        }
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum EngineErrorCode {
    Unimplemented = 0,
    InvalidState = 1,
    InvalidInput = 2,
    NotFound = 3,
    Conflict = 4,
    Timeout = 5,
    Busy = 6,
    Internal = 7,
}

impl Encode for EngineErrorCode {
    fn encode<E: bincode::enc::Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        (*self as u16).encode(encoder)
    }
}

impl<Context> Decode<Context> for EngineErrorCode {
    fn decode<D: bincode::de::Decoder<Context = Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let raw = u16::decode(decoder)?;
        Ok(match raw {
            0 => Self::Unimplemented,
            1 => Self::InvalidState,
            2 => Self::InvalidInput,
            3 => Self::NotFound,
            4 => Self::Conflict,
            5 => Self::Timeout,
            6 => Self::Busy,
            7 => Self::Internal,
            _ => Self::Internal,
        })
    }
}

bincode::impl_borrow_decode!(EngineErrorCode);

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct NodeRegistryPort {
    pub name: String,
    #[bincode(with_serde)]
    pub ty: JsonWire,
    pub source: Option<String>,
    #[bincode(with_serde)]
    #[serde(default)]
    pub const_value: Option<JsonWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct NodeRegistryFanInPort {
    pub prefix: String,
    #[serde(default)]
    pub start: u32,
    #[bincode(with_serde)]
    pub ty: JsonWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct NodeRegistryNode {
    pub id: String,
    pub label: Option<String>,
    pub plugin: Option<String>,
    pub feature_flags: Vec<String>,
    pub sync_groups: Vec<NodeSyncGroup>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub input_ports: Vec<NodeRegistryPort>,
    #[serde(default)]
    pub fanin_inputs: Vec<NodeRegistryFanInPort>,
    pub output_ports: Vec<NodeRegistryPort>,
    pub default_compute: String,
    #[bincode(with_serde)]
    pub metadata: std::collections::BTreeMap<String, JsonWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct NodeSyncGroup {
    pub name: String,
    pub policy: String,
    pub ports: Vec<String>,
    pub capacity: Option<usize>,
    pub backpressure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct TypeRegistryEntry {
    pub rust: String,
    #[bincode(with_serde)]
    pub ty: JsonWire,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, ToSchema)]
pub struct PluginCompatibility {
    pub filename: String,
    #[serde(default)]
    pub plugin_name: Option<String>,
    #[serde(default)]
    pub plugin_version: Option<String>,
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
    pub expected_daedalus_version: String,
    #[serde(default)]
    pub daedalus_version: Option<String>,
    pub expected_ffi_version: String,
    #[serde(default)]
    pub ffi_version: Option<String>,
    pub expected_abi_version: u32,
    #[serde(default)]
    pub abi_version: Option<u32>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct NodeRegistrySnapshot {
    pub plugins: Vec<String>,
    pub nodes: Vec<NodeRegistryNode>,
    pub types: Vec<TypeRegistryEntry>,
    #[serde(default)]
    pub plugin_compatibility: Vec<PluginCompatibility>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct PlannerDiagnosticSpan {
    pub pass: String,
    pub node: Option<String>,
    pub port: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct PlannerDiagnostic {
    pub code: String,
    pub message: String,
    pub span: PlannerDiagnosticSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct GraphValidationReport {
    pub ok: bool,
    pub diagnostics: Vec<PlannerDiagnostic>,
    #[serde(default)]
    pub gpu_segments: Vec<GraphGpuSegment>,
    #[serde(default)]
    pub gpu_edges: Vec<GraphGpuEdgeBufferInfo>,
    #[serde(default)]
    pub node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct GraphGpuSegment {
    pub buffer_id: usize,
    pub nodes: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct GraphGpuEdgeBufferInfo {
    pub edge_index: usize,
    pub gpu_fast_path: bool,
    pub buffer_id: Option<usize>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecordingContainer {
    Mp4,
    Raw,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordingCodec {
    H264,
    H265,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, ToSchema, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordingSource {
    #[default]
    Multiplex,
    Raw,
    Pipeline {
        #[serde(default)]
        #[bincode(with_serde)]
        pipeline_id: Option<Uuid>,
        #[serde(default)]
        output_key: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct LocalizationSolveSourceValue {
    pub source_id: String,
    #[serde(default)]
    #[bincode(with_serde)]
    pub value: Option<JsonWire>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct LocalizationSolveRequest {
    #[bincode(with_serde)]
    pub profile: crate::localization::config::LocalizationProfile,
    #[bincode(with_serde)]
    pub sources: Vec<crate::localization::config::LocalizationSourceConfig>,
    #[serde(default)]
    #[bincode(with_serde)]
    pub rig_poses: BTreeMap<String, crate::localization::types::LocalizationPose>,
    #[serde(default)]
    #[bincode(with_serde)]
    pub field_map: Option<crate::localization::maps::FieldMapDocument>,
    #[serde(default)]
    #[bincode(with_serde)]
    pub calibrations: BTreeMap<String, StreamCalibration>,
    #[serde(default)]
    #[bincode(with_serde)]
    pub source_values: Vec<LocalizationSolveSourceValue>,
    pub apply_field_origin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct LocalizationPipelineStatusRequest {
    pub profile_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct LocalizationPipelineGraphRequest {
    #[bincode(with_serde)]
    pub profile: crate::localization::config::LocalizationProfile,
    #[bincode(with_serde)]
    pub graph: JsonWire,
    #[serde(default)]
    pub graph_updated_at_ms: Option<i64>,
    #[serde(default)]
    pub template_mtime_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct LocalizationPipelineSampleRequest {
    #[bincode(with_serde)]
    pub profile: crate::localization::config::LocalizationProfile,
    #[bincode(with_serde)]
    pub sources: Vec<crate::localization::config::LocalizationSourceConfig>,
    #[bincode(with_serde)]
    pub graph: JsonWire,
    #[serde(default)]
    pub graph_updated_at_ms: Option<i64>,
    #[serde(default)]
    pub template_mtime_ms: Option<i64>,
    #[serde(default)]
    #[bincode(with_serde)]
    pub source_values: Vec<LocalizationSolveSourceValue>,
    pub output_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum EngineCommand {
    List {
        #[bincode(with_serde)]
        command_id: CommandId,
    },
    Start {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        manifest: Box<StreamManifest>,
    },
    /// Update encoder/decoder selection for a running stream without restarting capture.
    SetCodecs {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        decoder_id: Option<String>,
        #[bincode(with_serde)]
        encoder_id: Option<String>,
    },
    /// Update saved calibration for a running stream without restarting capture.
    SetCalibration {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        calibration: Option<StreamCalibration>,
    },
    /// Toggle guided calibration mode (pass-through preview + live detections output) without restarting capture.
    SetCalibrationMode {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        enabled: bool,
        #[bincode(with_serde)]
        dictionary: Option<String>,
        #[bincode(with_serde)]
        mode: Option<String>,
    },
    /// Solve camera intrinsics from calibration images + graph detections.
    SolveCalibration {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        request: super::CalibrationSolveRequest,
    },
    SolveLocalization {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        request: JsonWire,
    },
    GetLocalizationPipelineStatus {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        request: JsonWire,
    },
    ListLocalizationPipelineOutputs {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        request: JsonWire,
    },
    SampleLocalizationPipelineOutput {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        request: JsonWire,
    },
    Stop {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
    },
    SetControl {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        control_id: ControlId,
        #[bincode(with_serde)]
        value: CaptureControlValue,
    },
    GetControls {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
    },
    GetMetrics {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
    },
    /// Request a one-off JPEG snapshot from the latest decoded/graph-processed frame.
    ///
    /// This is intended for "take snapshot" flows where quality should be higher than the
    /// stream's live preview/output encoding settings.
    SnapshotJpeg {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        /// JPEG quality in range 1..=100 (clamped server-side).
        quality: u8,
        /// Optional source selector. When omitted, snapshot uses the current preview source.
        #[serde(default)]
        source: Option<RecordingSource>,
    },
    GetNodeRegistry {
        #[bincode(with_serde)]
        command_id: CommandId,
    },
    DiscoverDevices {
        #[bincode(with_serde)]
        command_id: CommandId,
    },
    RefreshNodeRegistry {
        #[bincode(with_serde)]
        command_id: CommandId,
    },
    ValidateGraph {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        graph: JsonWire,
        #[bincode(with_serde)]
        active_features: Vec<String>,
        enable_lints: bool,
    },
    /// Update the active pipeline graph for a running stream without restarting capture.
    SetGraph {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        graph: JsonWire,
        #[bincode(with_serde)]
        pipeline_id: Option<Uuid>,
        #[bincode(with_serde)]
        output: Option<String>,
    },
    /// Apply a graph patch (node constant overrides) to a running stream without rebuilding.
    SetGraphPatch {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        patch: JsonWire,
        #[bincode(with_serde)]
        pipeline_id: Option<Uuid>,
    },
    /// Update which graph output feeds the stream preview/encoder without restarting the stream.
    SetGraphOutput {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        output: Option<String>,
    },
    /// Update pipeline input values for a running stream without rebuilding the graph.
    SetPipelineInputs {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        pipeline_id: Option<Uuid>,
        #[bincode(with_serde)]
        inputs: BTreeMap<String, Option<JsonWire>>,
    },
    /// List host-bridge output ports (graph -> host) exposed by the running stream graph.
    ListGraphOutputs {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
    },
    /// List host-bridge output ports plus solved typing info for the running stream graph.
    // (folded into ListGraphOutputs)
    /// Fetch the latest JSON payload captured from a graph output port.
    GetGraphOutputSample {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        port: String,
    },
    /// Update multiplex layout (rows/columns/slot assignment) without restarting capture.
    SetPipelineLayout {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        layout: Option<StreamPipelineLayout>,
    },
    /// Update wiring between pipeline outputs and downstream pipeline inputs.
    SetPipelineWires {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        wires: Vec<StreamPipelineWire>,
    },
    /// Enable/disable perf counters collection for the running graph.
    SetGraphPerf {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        pipeline_id: Option<Uuid>,
        enabled: bool,
    },
    /// Reset rolling pipeline metrics (node timings/perf samples/flamegraph).
    ResetGraphMetrics {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        pipeline_id: Option<Uuid>,
    },
    /// Capture a CPU flamegraph over wall-clock time and store it on disk.
    CaptureGraphFlamegraph {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        pipeline_id: Option<Uuid>,
        duration_ms: u64,
    },
    /// Start recording a stream to a media file.
    StartRecording {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        source: RecordingSource,
        output_path: String,
        #[bincode(with_serde)]
        container: RecordingContainer,
        #[bincode(with_serde)]
        codec: RecordingCodec,
        #[bincode(with_serde)]
        duration_ms: Option<u64>,
        #[bincode(with_serde)]
        settings: Option<RecordingSettings>,
    },
    /// Stop an active recording for a stream.
    StopRecording {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
    },
    /// Capture the last N milliseconds from the shadow recorder buffer.
    CaptureShadowRecording {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        output_path: String,
        #[bincode(with_serde)]
        container: RecordingContainer,
        window_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, ToSchema)]
pub struct FrameRate {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, Default, ToSchema)]
pub struct RecordingSettings {
    /// Target recording FPS (best-effort frame dropping).
    #[serde(default)]
    pub fps: Option<f32>,
    /// Optional target bitrate in bits per second (transcodes MP4 output).
    #[serde(default)]
    pub bitrate_bps: Option<u64>,
    /// Optional GOP size (transcodes MP4 output).
    #[serde(default)]
    pub gop: Option<i32>,
    /// Optional CRF quality (0-51, lower = higher quality). Only used when transcoding.
    #[serde(default)]
    pub quality: Option<u8>,
    /// Optional maximum output width (transcodes MP4 output).
    #[serde(default)]
    pub max_width: Option<u32>,
    /// Optional maximum output height (transcodes MP4 output).
    #[serde(default)]
    pub max_height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, ToSchema)]
pub struct ResolutionHint {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, Default, ToSchema)]
pub struct EncoderSettings {
    #[serde(default)]
    pub bitrate: Option<u64>,
    #[serde(default)]
    pub gop: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_encoder_frame_rate")]
    pub framerate: Option<FrameRate>,
    #[serde(default)]
    pub thread_count: Option<usize>,
    #[serde(default)]
    pub output_resolution: Option<ResolutionHint>,
    /// Optional soft limit for decode FPS; frames above this are dropped before encoding.
    #[serde(default)]
    pub decode_fps_limit: Option<f64>,
}

fn deserialize_encoder_frame_rate<'de, D>(deserializer: D) -> Result<Option<FrameRate>, D::Error>
where
    D: Deserializer<'de>,
{
    // Preserve exact binary compatibility for IPC/bincode.
    if !deserializer.is_human_readable() {
        return Option::<FrameRate>::deserialize(deserializer);
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FrameRateWire {
        Rational(FrameRate),
        LegacyFps { fps: f64 },
    }

    let parsed = Option::<FrameRateWire>::deserialize(deserializer)?;
    match parsed {
        None => Ok(None),
        Some(FrameRateWire::Rational(rate)) => Ok(Some(rate)),
        Some(FrameRateWire::LegacyFps { fps }) => {
            if !fps.is_finite() || fps <= 0.0 {
                return Err(serde::de::Error::custom("framerate.fps must be a positive finite number"));
            }
            const SCALE: u32 = 1000;
            let scaled = (fps * f64::from(SCALE)).round();
            if !scaled.is_finite() || scaled <= 0.0 || scaled > u32::MAX as f64 {
                return Err(serde::de::Error::custom("framerate.fps is out of range"));
            }
            let numerator = scaled as u32;
            let denominator = SCALE;
            let divisor = gcd_u32(numerator, denominator);
            Ok(Some(FrameRate { numerator: numerator / divisor, denominator: denominator / divisor }))
        }
    }
}

fn gcd_u32(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    if a == 0 {
        1
    } else {
        a
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct DecoderSettings {
    /// Optional decoder thread count (codec-dependent).
    #[serde(default)]
    pub thread_count: Option<usize>,
    /// Optional soft limit for decode FPS; frames above this are dropped before decode/graph.
    #[serde(default)]
    pub fps_limit: Option<f64>,
    /// Optional rotation applied after decode (0/90/180/270 degrees).
    #[serde(default)]
    pub rotation_degrees: Option<i32>,
    /// Optional horizontal mirror applied after decode.
    #[serde(default)]
    pub mirror_horizontal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum EngineEvent {
    Ack {
        #[bincode(with_serde)]
        command_id: CommandId,
        ok: bool,
    },
    Nack {
        #[bincode(with_serde)]
        command_id: CommandId,
        code: EngineErrorCode,
        reason: String,
    },
    StreamList {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        streams: Vec<StreamSummary>,
    },
    Started {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        descriptor: CaptureDescriptor,
    },
    Stopped {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
    },
    Controls {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        controls: Vec<CaptureControlInfo>,
    },
    Metrics {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        metrics: StreamMetrics,
    },
    SnapshotJpeg {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        quality: u8,
        #[bincode(with_serde)]
        bytes: Vec<u8>,
    },
    GraphOutputs {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        outputs: Vec<GraphOutputPortDescriptor>,
    },
    GraphOutputSample {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        stream_id: Uuid,
        port: String,
        #[bincode(with_serde)]
        value: JsonWire,
    },
    /// Unsolicited metrics update broadcast by the engine.
    MetricsUpdate {
        #[bincode(with_serde)]
        stream_id: Uuid,
        #[bincode(with_serde)]
        metrics: StreamMetrics,
    },
    NodeRegistry {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        snapshot: NodeRegistrySnapshot,
    },
    Discovery {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        discovery: crate::capture::DiscoveryResult,
    },
    GraphValidation {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        report: GraphValidationReport,
    },
    CalibrationSolved {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        response: super::CalibrationSolveResponse,
    },
    LocalizationSolved {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        response: JsonWire,
    },
    LocalizationPipelineStatus {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        response: JsonWire,
    },
    LocalizationPipelineOutputs {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        outputs: Vec<String>,
    },
    LocalizationPipelineOutputSample {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        response: JsonWire,
    },
}

impl EngineCommand {
    pub fn command_id(&self) -> Option<CommandId> {
        Some(match self {
            EngineCommand::List { command_id }
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

impl ServerEvent for EngineEvent {
    fn message_kind(&self) -> MessageKind {
        match self {
            Self::Ack { .. }
            | Self::Nack { .. }
            | Self::Started { .. }
            | Self::Stopped { .. }
            | Self::Controls { .. }
            | Self::Metrics { .. }
            | Self::SnapshotJpeg { .. }
            | Self::GraphOutputs { .. }
            | Self::GraphOutputSample { .. }
            | Self::StreamList { .. }
            | Self::MetricsUpdate { .. }
            | Self::NodeRegistry { .. }
            | Self::Discovery { .. }
            | Self::GraphValidation { .. }
            | Self::CalibrationSolved { .. }
            | Self::LocalizationSolved { .. }
            | Self::LocalizationPipelineStatus { .. }
            | Self::LocalizationPipelineOutputs { .. }
            | Self::LocalizationPipelineOutputSample { .. } => MessageKind::Event,
        }
    }

    fn as_control(&self) -> Option<&ControlEvent> {
        None
    }
}

impl From<ControlEvent> for EngineEvent {
    fn from(event: ControlEvent) -> Self {
        match event {
            ControlEvent::Ack(ack) => EngineEvent::Ack { command_id: ack.command_id, ok: true },
            ControlEvent::Nack(nack) => EngineEvent::Nack { command_id: nack.command_id, code: EngineErrorCode::InvalidState, reason: nack.reason },
        }
    }
}

impl EngineEvent {
    pub fn command_id(&self) -> Option<CommandId> {
        match self {
            EngineEvent::Ack { command_id, .. }
            | EngineEvent::Nack { command_id, .. }
            | EngineEvent::StreamList { command_id, .. }
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamPipelineBinding {
    pub pipeline_id: Uuid,
    #[serde(default)]
    pub pipeline_graph: Option<JsonWire>,
    /// Optional host-bridge output port to use as the pipeline's frame output.
    #[serde(default)]
    pub pipeline_output: Option<String>,
    /// Optional per-stream patch applied on top of the pipeline graph.
    #[serde(default)]
    pub pipeline_patch: Option<JsonWire>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamPipelineGridSlot {
    pub row: u8,
    pub column: u8,
    #[serde(default)]
    pub pipeline_id: Option<Uuid>,
    /// Optional host-bridge output port override for this slot.
    #[serde(default)]
    pub output_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamPipelineLayout {
    pub rows: u8,
    pub columns: u8,
    #[serde(default)]
    pub slots: Vec<StreamPipelineGridSlot>,
}

/// Endpoint in the multiplex pipeline wiring graph.
///
/// This identifies a specific pipeline *instance* (pipeline ID + optional layout `output_key`)
/// and a port name on that instance.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamPipelineEndpoint {
    pub pipeline_id: Uuid,
    /// Optional instance discriminator when a pipeline appears multiple times with different
    /// layout `output_key` values.
    #[serde(default)]
    pub output_key: Option<String>,
    /// Port name on the pipeline instance. For pipeline frame input, this is typically `frame`.
    /// For pipeline outputs, this can be omitted to use the instance's selected output port.
    #[serde(default)]
    pub port: Option<String>,
}

/// Wire a port from one pipeline instance into an input port on another pipeline instance.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamPipelineWire {
    pub from: StreamPipelineEndpoint,
    pub to: StreamPipelineEndpoint,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamCalibration {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
    pub k1: f64,
    pub k2: f64,
    pub p1: f64,
    pub p2: f64,
    pub k3: f64,
    pub undistort_iters: i64,
    #[serde(default)]
    pub lens_model: LensModel,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PoseVector {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct PoseRotation {
    /// Degrees.
    pub roll: f64,
    /// Degrees.
    pub pitch: f64,
    /// Degrees.
    pub yaw: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RigPose {
    pub translation: PoseVector,
    pub rotation: PoseRotation,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamManifest {
    pub identity: DeviceIdentity,
    pub capture: CaptureConfig,
    #[serde(default = "default_host_buffer")]
    pub host_buffer: usize,
    /// Internal streams are created by the system for tasks like benchmarking and should not
    /// appear in user-facing stream lists / registration UX.
    #[serde(default)]
    pub internal: bool,
    /// When set to `false`, force the stream to run without any pipeline graph (raw frames).
    ///
    /// This exists because JSON `null` / absent fields are indistinguishable for `Option<T>` and we
    /// need an explicit way for clients to clear a previously persisted pipeline.
    #[serde(default)]
    pub pipeline_enabled: Option<bool>,
    /// Optional additional pipeline graphs to run in multiplex/debug view.
    #[serde(default)]
    pub pipelines: Vec<StreamPipelineBinding>,
    /// Active pipeline ID when `pipelines` is set.
    #[serde(default)]
    pub active_pipeline_id: Option<Uuid>,
    /// Selected host output port for the active pipeline when `pipelines` is set.
    #[serde(default)]
    pub active_pipeline_output: Option<String>,
    /// Optional layout for multiplex rendering (rows/columns + slot assignments).
    #[serde(default)]
    pub pipeline_layout: Option<StreamPipelineLayout>,
    /// Optional wiring between pipeline outputs and downstream pipeline inputs.
    #[serde(default)]
    pub pipeline_wires: Vec<StreamPipelineWire>,
    /// Persisted host-bridge input values applied to the active pipeline graph at startup/rebuild.
    ///
    /// Used for stream-level controls like ROI crop, crosshair, and ordering mode.
    #[serde(default)]
    pub pipeline_host_inputs: BTreeMap<String, JsonWire>,
    /// Optional saved calibration intrinsics/distortion coefficients for this camera.
    #[serde(default)]
    pub calibration: Option<StreamCalibration>,
    /// Optional rig pose (translation + rotation) for this camera.
    #[serde(default)]
    pub pose: Option<RigPose>,
    /// Explicit toggle for encoder enable/disable.
    ///
    /// This exists because JSON `null` / absent fields are indistinguishable for `Option<T>`.
    /// Clients must be able to explicitly disable the encoder without having the API silently
    /// inherit a previously persisted encoder selection when restarting the same capture format.
    #[serde(default)]
    pub encoder_enabled: Option<bool>,
    #[serde(default)]
    pub encoder_id: Option<String>,
    /// Explicit toggle for decoder enable/disable.
    ///
    /// This exists because JSON `null` / absent fields are indistinguishable for `Option<T>`.
    /// Clients must be able to explicitly disable the decoder without having the API silently
    /// inherit a previously persisted decoder selection when restarting the same capture format.
    #[serde(default)]
    pub decoder_enabled: Option<bool>,
    #[serde(default)]
    pub decoder_id: Option<String>,
    #[serde(default)]
    pub encoder_settings: Option<EncoderSettings>,
    #[serde(default)]
    pub decoder_settings: Option<DecoderSettings>,
    /// Enable the rolling shadow recorder buffer used for capture-last clips.
    #[serde(default = "default_shadow_recorder_enabled")]
    pub shadow_recorder_enabled: bool,
    #[serde(default)]
    pub start_on_boot: bool,
}

impl StreamManifest {
    pub fn host_buffer(&self) -> usize {
        let requested = if self.host_buffer == 0 { default_host_buffer() } else { self.host_buffer };
        let max = max_host_buffer();
        if requested > max {
            tracing::warn!(requested, max, "host buffer too large; clamping to avoid excessive memory use");
            max
        } else {
            requested
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSummary {
    pub stream_id: Uuid,
    pub descriptor: CaptureDescriptor,
    pub manifest: StreamManifest,
    #[serde(default)]
    pub status: StreamStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum StreamState {
    #[default]
    Running,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamStatus {
    #[serde(default)]
    pub state: StreamState,
    #[serde(default)]
    pub started_at_ms: Option<u64>,
    #[serde(default)]
    pub disabled_since_ms: Option<u64>,
    #[serde(default)]
    pub disabled_reason: Option<String>,
    #[serde(default)]
    pub recording_active: bool,
    #[serde(default)]
    pub recording_since_ms: Option<u64>,
}

pub(crate) fn default_host_buffer() -> usize {
    // The host bridge buffers full frames for late subscribers. At full resolution this can
    // balloon RSS quickly (e.g. RGBA at 2K+). Default small to keep memory predictable; users can
    // still override via `HELIOS_HOST_BUFFER` or per-stream `host_buffer`.
    let requested = env::var("HELIOS_HOST_BUFFER").ok().and_then(|v| v.parse().ok()).filter(|v| *v > 0).unwrap_or(2);
    requested.min(max_host_buffer())
}

pub(crate) fn default_shadow_recorder_enabled() -> bool {
    // Feature-gated for now; enable explicitly.
    let raw = env::var("HELIOS_ENABLE_SHADOW_RECORDER").ok().unwrap_or_default();
    let v = raw.trim().to_ascii_lowercase();
    matches!(v.as_str(), "1" | "true" | "yes" | "y" | "on" | "enabled")
}

fn max_host_buffer() -> usize {
    env::var("HELIOS_HOST_BUFFER_MAX").ok().and_then(|v| v.parse().ok()).filter(|v| *v > 0).unwrap_or(64)
}
