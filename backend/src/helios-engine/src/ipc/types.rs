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
use styx::BackendKind;
use styx::codec::{CodecKind, CodecRegistry};
use styx::prelude::{FourCc, Resolution};

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

fn default_enable_lints() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphValidationHelperRequest {
    pub graph: JsonWire,
    #[serde(default)]
    pub active_features: Vec<String>,
    #[serde(default = "default_enable_lints")]
    pub enable_lints: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GraphValidationHelperResponse {
    Report { report: GraphValidationReport },
    Error { code: EngineErrorCode, reason: String },
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
        manifest: Box<ResolvedStreamConfig>,
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
        fresh: bool,
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

pub type RequestedStreamConfig = StreamManifest;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedEncoderConfig {
    pub enabled: bool,
    #[serde(default)]
    pub codec_id: Option<String>,
    #[serde(default)]
    pub settings: Option<EncoderSettings>,
    #[serde(default)]
    pub settings_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDecoderConfig {
    pub enabled: bool,
    #[serde(default)]
    pub codec_id: Option<String>,
    #[serde(default)]
    pub settings: Option<DecoderSettings>,
    #[serde(default)]
    pub settings_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedStreamConfig {
    pub identity: DeviceIdentity,
    pub capture: CaptureConfig,
    pub host_buffer: usize,
    pub internal: bool,
    pub pipeline_enabled: bool,
    #[serde(default)]
    pub pipelines: Vec<StreamPipelineBinding>,
    #[serde(default)]
    pub active_pipeline_id: Option<Uuid>,
    #[serde(default)]
    pub active_pipeline_output: Option<String>,
    #[serde(default)]
    pub pipeline_layout: Option<StreamPipelineLayout>,
    #[serde(default)]
    pub pipeline_wires: Vec<StreamPipelineWire>,
    #[serde(default)]
    pub pipeline_host_inputs: BTreeMap<String, JsonWire>,
    #[serde(default)]
    pub calibration: Option<StreamCalibration>,
    #[serde(default)]
    pub pose: Option<RigPose>,
    #[serde(default)]
    pub encoder: ResolvedEncoderConfig,
    #[serde(default)]
    pub decoder: ResolvedDecoderConfig,
    pub preview_jpeg_quality: u8,
    pub shadow_recorder_enabled: bool,
    pub start_on_boot: bool,
}

#[derive(Debug, Clone)]
pub enum RequestedEncoderConfig {
    Disabled,
    Enabled {
        id: Option<String>,
        settings: Option<EncoderSettings>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RequestedEncoderConfigSchema {
    Disabled,
    Enabled {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        settings: Option<EncoderSettings>,
    },
}

impl Default for RequestedEncoderConfig {
    fn default() -> Self {
        Self::Enabled { id: None, settings: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum RequestedEncoderConfigBinaryWire {
    Disabled,
    Enabled {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        settings: Option<EncoderSettings>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum RequestedEncoderConfigHumanWire {
    Disabled,
    Enabled {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        settings: Option<EncoderSettings>,
    },
}

impl RequestedEncoderConfig {
    pub fn disabled() -> Self {
        Self::Disabled
    }

    pub fn enabled(id: Option<String>, settings: Option<EncoderSettings>) -> Self {
        Self::Enabled { id, settings }
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Disabled => None,
            Self::Enabled { id, .. } => id.as_deref(),
        }
    }

    pub fn settings(&self) -> Option<&EncoderSettings> {
        match self {
            Self::Disabled => None,
            Self::Enabled { settings, .. } => settings.as_ref(),
        }
    }

    pub fn ensure_id(&mut self, id: Option<String>) {
        match self {
            Self::Disabled => {}
            Self::Enabled { id: existing, .. } => {
                if existing.is_none() {
                    *existing = normalized_codec_selector(id.as_deref());
                }
            }
        }
    }

    pub fn ensure_settings(&mut self, settings: Option<EncoderSettings>) {
        match self {
            Self::Disabled => {}
            Self::Enabled { settings: existing, .. } => {
                if existing.is_none() {
                    *existing = settings;
                }
            }
        }
    }

    fn from_legacy(enabled: Option<bool>, id: Option<String>, settings: Option<EncoderSettings>) -> Result<Self, String> {
        let normalized_id = normalized_codec_selector(id.as_deref());
        if enabled == Some(false) {
            if normalized_id.is_some() || settings.is_some() {
                return Err("legacy encoder_enabled=false may not be combined with encoder_id or encoder_settings".to_string());
            }
            return Ok(Self::Disabled);
        }
        Ok(Self::enabled(normalized_id, settings))
    }
}

impl From<RequestedEncoderConfig> for RequestedEncoderConfigBinaryWire {
    fn from(value: RequestedEncoderConfig) -> Self {
        match value {
            RequestedEncoderConfig::Disabled => Self::Disabled,
            RequestedEncoderConfig::Enabled { id, settings } => Self::Enabled { id, settings },
        }
    }
}

impl From<RequestedEncoderConfigBinaryWire> for RequestedEncoderConfig {
    fn from(value: RequestedEncoderConfigBinaryWire) -> Self {
        match value {
            RequestedEncoderConfigBinaryWire::Disabled => Self::Disabled,
            RequestedEncoderConfigBinaryWire::Enabled { id, settings } => Self::Enabled { id, settings },
        }
    }
}

impl From<RequestedEncoderConfig> for RequestedEncoderConfigHumanWire {
    fn from(value: RequestedEncoderConfig) -> Self {
        match value {
            RequestedEncoderConfig::Disabled => Self::Disabled,
            RequestedEncoderConfig::Enabled { id, settings } => Self::Enabled { id, settings },
        }
    }
}

impl From<RequestedEncoderConfigHumanWire> for RequestedEncoderConfig {
    fn from(value: RequestedEncoderConfigHumanWire) -> Self {
        match value {
            RequestedEncoderConfigHumanWire::Disabled => Self::Disabled,
            RequestedEncoderConfigHumanWire::Enabled { id, settings } => Self::Enabled { id, settings },
        }
    }
}

impl Serialize for RequestedEncoderConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            RequestedEncoderConfigHumanWire::from(self.clone()).serialize(serializer)
        } else {
            RequestedEncoderConfigBinaryWire::from(self.clone()).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for RequestedEncoderConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            Ok(RequestedEncoderConfigHumanWire::deserialize(deserializer)?.into())
        } else {
            Ok(RequestedEncoderConfigBinaryWire::deserialize(deserializer)?.into())
        }
    }
}

#[derive(Debug, Clone)]
pub enum RequestedDecoderConfig {
    Disabled,
    Enabled {
        id: Option<String>,
        settings: Option<DecoderSettings>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RequestedDecoderConfigSchema {
    Disabled,
    Enabled {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        settings: Option<DecoderSettings>,
    },
}

impl Default for RequestedDecoderConfig {
    fn default() -> Self {
        Self::Enabled { id: None, settings: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum RequestedDecoderConfigBinaryWire {
    Disabled,
    Enabled {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        settings: Option<DecoderSettings>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum RequestedDecoderConfigHumanWire {
    Disabled,
    Enabled {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        settings: Option<DecoderSettings>,
    },
}

impl RequestedDecoderConfig {
    pub fn disabled() -> Self {
        Self::Disabled
    }

    pub fn enabled(id: Option<String>, settings: Option<DecoderSettings>) -> Self {
        Self::Enabled { id, settings }
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Disabled => None,
            Self::Enabled { id, .. } => id.as_deref(),
        }
    }

    pub fn settings(&self) -> Option<&DecoderSettings> {
        match self {
            Self::Disabled => None,
            Self::Enabled { settings, .. } => settings.as_ref(),
        }
    }

    pub fn ensure_id(&mut self, id: Option<String>) {
        match self {
            Self::Disabled => {}
            Self::Enabled { id: existing, .. } => {
                if existing.is_none() {
                    *existing = normalized_codec_selector(id.as_deref());
                }
            }
        }
    }

    pub fn ensure_settings(&mut self, settings: Option<DecoderSettings>) {
        match self {
            Self::Disabled => {}
            Self::Enabled { settings: existing, .. } => {
                if existing.is_none() {
                    *existing = settings;
                }
            }
        }
    }

    fn from_legacy(enabled: Option<bool>, id: Option<String>, settings: Option<DecoderSettings>) -> Result<Self, String> {
        let normalized_id = normalized_codec_selector(id.as_deref());
        if enabled == Some(false) {
            if normalized_id.is_some() || settings.is_some() {
                return Err("legacy decoder_enabled=false may not be combined with decoder_id or decoder_settings".to_string());
            }
            return Ok(Self::Disabled);
        }
        Ok(Self::enabled(normalized_id, settings))
    }
}

impl From<RequestedDecoderConfig> for RequestedDecoderConfigBinaryWire {
    fn from(value: RequestedDecoderConfig) -> Self {
        match value {
            RequestedDecoderConfig::Disabled => Self::Disabled,
            RequestedDecoderConfig::Enabled { id, settings } => Self::Enabled { id, settings },
        }
    }
}

impl From<RequestedDecoderConfigBinaryWire> for RequestedDecoderConfig {
    fn from(value: RequestedDecoderConfigBinaryWire) -> Self {
        match value {
            RequestedDecoderConfigBinaryWire::Disabled => Self::Disabled,
            RequestedDecoderConfigBinaryWire::Enabled { id, settings } => Self::Enabled { id, settings },
        }
    }
}

impl From<RequestedDecoderConfig> for RequestedDecoderConfigHumanWire {
    fn from(value: RequestedDecoderConfig) -> Self {
        match value {
            RequestedDecoderConfig::Disabled => Self::Disabled,
            RequestedDecoderConfig::Enabled { id, settings } => Self::Enabled { id, settings },
        }
    }
}

impl From<RequestedDecoderConfigHumanWire> for RequestedDecoderConfig {
    fn from(value: RequestedDecoderConfigHumanWire) -> Self {
        match value {
            RequestedDecoderConfigHumanWire::Disabled => Self::Disabled,
            RequestedDecoderConfigHumanWire::Enabled { id, settings } => Self::Enabled { id, settings },
        }
    }
}

impl Serialize for RequestedDecoderConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            RequestedDecoderConfigHumanWire::from(self.clone()).serialize(serializer)
        } else {
            RequestedDecoderConfigBinaryWire::from(self.clone()).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for RequestedDecoderConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            Ok(RequestedDecoderConfigHumanWire::deserialize(deserializer)?.into())
        } else {
            Ok(RequestedDecoderConfigBinaryWire::deserialize(deserializer)?.into())
        }
    }
}

fn canonical_requested_host_buffer(value: usize) -> usize {
    if value == 0 { default_host_buffer() } else { value }
}

fn canonical_requested_pipeline_enabled(
    value: Option<bool>,
    pipelines: &[StreamPipelineBinding],
    active_pipeline_id: Option<Uuid>,
    active_pipeline_output: Option<&str>,
    pipeline_layout: Option<&StreamPipelineLayout>,
    pipeline_wires: &[StreamPipelineWire],
) -> bool {
    value.unwrap_or_else(|| {
        !pipelines.is_empty()
            || active_pipeline_id.is_some()
            || active_pipeline_output.is_some_and(|value| !value.trim().is_empty())
            || pipeline_layout.is_some()
            || !pipeline_wires.is_empty()
    })
}

fn default_requested_preview_jpeg_quality(encoder: &RequestedEncoderConfig) -> u8 {
    if encoder.is_disabled() {
        default_preview_jpeg_quality_override().unwrap_or(DEFAULT_PREVIEW_JPEG_QUALITY).clamp(1, 100)
    } else {
        DEFAULT_STREAM_PREVIEW_JPEG_QUALITY
    }
}

fn canonical_requested_preview_jpeg_quality(value: Option<u8>, encoder: &RequestedEncoderConfig) -> u8 {
    value.map(|value| value.clamp(1, 100)).unwrap_or_else(|| default_requested_preview_jpeg_quality(encoder))
}

#[derive(Debug, Clone, ToSchema)]
pub struct StreamManifest {
    pub identity: DeviceIdentity,
    pub capture: CaptureConfig,
    pub host_buffer: usize,
    /// Internal streams are created by the system for tasks like benchmarking and should not
    /// appear in user-facing stream lists / registration UX.
    pub internal: bool,
    /// When set to `false`, force the stream to run without any pipeline graph (raw frames).
    pub pipeline_enabled: bool,
    /// Optional additional pipeline graphs to run in multiplex/debug view.
    pub pipelines: Vec<StreamPipelineBinding>,
    /// Active pipeline ID when `pipelines` is set.
    pub active_pipeline_id: Option<Uuid>,
    /// Selected host output port for the active pipeline when `pipelines` is set.
    pub active_pipeline_output: Option<String>,
    /// Optional layout for multiplex rendering (rows/columns + slot assignments).
    pub pipeline_layout: Option<StreamPipelineLayout>,
    /// Optional wiring between pipeline outputs and downstream pipeline inputs.
    pub pipeline_wires: Vec<StreamPipelineWire>,
    /// Persisted host-bridge input values applied to the active pipeline graph at startup/rebuild.
    ///
    /// Used for stream-level controls like ROI crop, crosshair, and ordering mode.
    pub pipeline_host_inputs: BTreeMap<String, JsonWire>,
    /// Optional saved calibration intrinsics/distortion coefficients for this camera.
    pub calibration: Option<StreamCalibration>,
    /// Optional rig pose (translation + rotation) for this camera.
    pub pose: Option<RigPose>,
    #[schema(value_type = RequestedEncoderConfigSchema)]
    pub encoder: RequestedEncoderConfig,
    #[schema(value_type = RequestedDecoderConfigSchema)]
    pub decoder: RequestedDecoderConfig,
    pub preview_jpeg_quality: u8,
    /// Enable the rolling shadow recorder buffer used for capture-last clips.
    pub shadow_recorder_enabled: bool,
    pub start_on_boot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamManifestBinaryWire {
    pub identity: DeviceIdentity,
    pub capture: CaptureConfig,
    #[serde(default = "default_host_buffer")]
    pub host_buffer: usize,
    #[serde(default)]
    pub internal: bool,
    #[serde(default)]
    pub pipeline_enabled: Option<bool>,
    #[serde(default)]
    pub pipelines: Vec<StreamPipelineBinding>,
    #[serde(default)]
    pub active_pipeline_id: Option<Uuid>,
    #[serde(default)]
    pub active_pipeline_output: Option<String>,
    #[serde(default)]
    pub pipeline_layout: Option<StreamPipelineLayout>,
    #[serde(default)]
    pub pipeline_wires: Vec<StreamPipelineWire>,
    #[serde(default)]
    pub pipeline_host_inputs: BTreeMap<String, JsonWire>,
    #[serde(default)]
    pub calibration: Option<StreamCalibration>,
    #[serde(default)]
    pub pose: Option<RigPose>,
    #[serde(default)]
    pub encoder: RequestedEncoderConfig,
    #[serde(default)]
    pub decoder: RequestedDecoderConfig,
    #[serde(default)]
    pub preview_jpeg_quality: Option<u8>,
    #[serde(default = "default_shadow_recorder_enabled")]
    pub shadow_recorder_enabled: bool,
    #[serde(default)]
    pub start_on_boot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamManifestHumanWire {
    pub identity: DeviceIdentity,
    pub capture: CaptureConfig,
    #[serde(default)]
    pub host_buffer: Option<usize>,
    #[serde(default)]
    pub internal: bool,
    #[serde(default)]
    pub pipeline_enabled: Option<bool>,
    #[serde(default)]
    pub pipelines: Vec<StreamPipelineBinding>,
    #[serde(default)]
    pub active_pipeline_id: Option<Uuid>,
    #[serde(default)]
    pub active_pipeline_output: Option<String>,
    #[serde(default)]
    pub pipeline_layout: Option<StreamPipelineLayout>,
    #[serde(default)]
    pub pipeline_wires: Vec<StreamPipelineWire>,
    #[serde(default)]
    pub pipeline_host_inputs: BTreeMap<String, JsonWire>,
    #[serde(default)]
    pub calibration: Option<StreamCalibration>,
    #[serde(default)]
    pub pose: Option<RigPose>,
    #[serde(default)]
    pub encoder: Option<RequestedEncoderConfig>,
    #[serde(default)]
    pub encoder_enabled: Option<bool>,
    #[serde(default)]
    pub encoder_id: Option<String>,
    #[serde(default)]
    pub encoder_settings: Option<EncoderSettings>,
    #[serde(default)]
    pub decoder: Option<RequestedDecoderConfig>,
    #[serde(default)]
    pub decoder_enabled: Option<bool>,
    #[serde(default)]
    pub decoder_id: Option<String>,
    #[serde(default)]
    pub decoder_settings: Option<DecoderSettings>,
    #[serde(default)]
    pub preview_jpeg_quality: Option<u8>,
    #[serde(default = "default_shadow_recorder_enabled")]
    pub shadow_recorder_enabled: bool,
    #[serde(default)]
    pub start_on_boot: bool,
}

impl From<StreamManifestBinaryWire> for StreamManifest {
    fn from(value: StreamManifestBinaryWire) -> Self {
        let pipeline_enabled = canonical_requested_pipeline_enabled(
            value.pipeline_enabled,
            &value.pipelines,
            value.active_pipeline_id,
            value.active_pipeline_output.as_deref(),
            value.pipeline_layout.as_ref(),
            &value.pipeline_wires,
        );
        let host_buffer = canonical_requested_host_buffer(value.host_buffer);
        let preview_jpeg_quality = canonical_requested_preview_jpeg_quality(value.preview_jpeg_quality, &value.encoder);
        Self {
            identity: value.identity,
            capture: value.capture,
            host_buffer,
            internal: value.internal,
            pipeline_enabled,
            pipelines: value.pipelines,
            active_pipeline_id: value.active_pipeline_id,
            active_pipeline_output: value.active_pipeline_output,
            pipeline_layout: value.pipeline_layout,
            pipeline_wires: value.pipeline_wires,
            pipeline_host_inputs: value.pipeline_host_inputs,
            calibration: value.calibration,
            pose: value.pose,
            encoder: value.encoder,
            decoder: value.decoder,
            preview_jpeg_quality,
            shadow_recorder_enabled: value.shadow_recorder_enabled,
            start_on_boot: value.start_on_boot,
        }
    }
}

impl From<StreamManifest> for StreamManifestBinaryWire {
    fn from(value: StreamManifest) -> Self {
        Self {
            identity: value.identity,
            capture: value.capture,
            host_buffer: value.host_buffer,
            internal: value.internal,
            pipeline_enabled: Some(value.pipeline_enabled),
            pipelines: value.pipelines,
            active_pipeline_id: value.active_pipeline_id,
            active_pipeline_output: value.active_pipeline_output,
            pipeline_layout: value.pipeline_layout,
            pipeline_wires: value.pipeline_wires,
            pipeline_host_inputs: value.pipeline_host_inputs,
            calibration: value.calibration,
            pose: value.pose,
            encoder: value.encoder,
            decoder: value.decoder,
            preview_jpeg_quality: Some(value.preview_jpeg_quality),
            shadow_recorder_enabled: value.shadow_recorder_enabled,
            start_on_boot: value.start_on_boot,
        }
    }
}

impl TryFrom<StreamManifestHumanWire> for StreamManifest {
    type Error = String;

    fn try_from(value: StreamManifestHumanWire) -> Result<Self, Self::Error> {
        let encoder = match (value.encoder, value.encoder_enabled, value.encoder_id, value.encoder_settings) {
            (Some(encoder), None, None, None) => encoder,
            (Some(_), _, _, _) => {
                return Err("stream manifest may not mix `encoder` with legacy `encoder_enabled`, `encoder_id`, or `encoder_settings` fields".to_string())
            }
            (None, enabled, id, settings) => RequestedEncoderConfig::from_legacy(enabled, id, settings)?,
        };
        let decoder = match (value.decoder, value.decoder_enabled, value.decoder_id, value.decoder_settings) {
            (Some(decoder), None, None, None) => decoder,
            (Some(_), _, _, _) => {
                return Err("stream manifest may not mix `decoder` with legacy `decoder_enabled`, `decoder_id`, or `decoder_settings` fields".to_string())
            }
            (None, enabled, id, settings) => RequestedDecoderConfig::from_legacy(enabled, id, settings)?,
        };
        let pipeline_enabled = canonical_requested_pipeline_enabled(
            value.pipeline_enabled,
            &value.pipelines,
            value.active_pipeline_id,
            value.active_pipeline_output.as_deref(),
            value.pipeline_layout.as_ref(),
            &value.pipeline_wires,
        );
        let host_buffer = canonical_requested_host_buffer(value.host_buffer.unwrap_or_else(default_host_buffer));
        let preview_jpeg_quality = canonical_requested_preview_jpeg_quality(value.preview_jpeg_quality, &encoder);

        Ok(Self {
            identity: value.identity,
            capture: value.capture,
            host_buffer,
            internal: value.internal,
            pipeline_enabled,
            pipelines: value.pipelines,
            active_pipeline_id: value.active_pipeline_id,
            active_pipeline_output: value.active_pipeline_output,
            pipeline_layout: value.pipeline_layout,
            pipeline_wires: value.pipeline_wires,
            pipeline_host_inputs: value.pipeline_host_inputs,
            calibration: value.calibration,
            pose: value.pose,
            encoder,
            decoder,
            preview_jpeg_quality,
            shadow_recorder_enabled: value.shadow_recorder_enabled,
            start_on_boot: value.start_on_boot,
        })
    }
}

impl From<StreamManifest> for StreamManifestHumanWire {
    fn from(value: StreamManifest) -> Self {
        Self {
            identity: value.identity,
            capture: value.capture,
            host_buffer: Some(value.host_buffer),
            internal: value.internal,
            pipeline_enabled: Some(value.pipeline_enabled),
            pipelines: value.pipelines,
            active_pipeline_id: value.active_pipeline_id,
            active_pipeline_output: value.active_pipeline_output,
            pipeline_layout: value.pipeline_layout,
            pipeline_wires: value.pipeline_wires,
            pipeline_host_inputs: value.pipeline_host_inputs,
            calibration: value.calibration,
            pose: value.pose,
            encoder: Some(value.encoder),
            encoder_enabled: None,
            encoder_id: None,
            encoder_settings: None,
            decoder: Some(value.decoder),
            decoder_enabled: None,
            decoder_id: None,
            decoder_settings: None,
            preview_jpeg_quality: Some(value.preview_jpeg_quality),
            shadow_recorder_enabled: value.shadow_recorder_enabled,
            start_on_boot: value.start_on_boot,
        }
    }
}

impl Serialize for StreamManifest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            StreamManifestHumanWire::from(self.clone()).serialize(serializer)
        } else {
            StreamManifestBinaryWire::from(self.clone()).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for StreamManifest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let wire = StreamManifestHumanWire::deserialize(deserializer)?;
            Self::try_from(wire).map_err(serde::de::Error::custom)
        } else {
            Ok(StreamManifestBinaryWire::deserialize(deserializer)?.into())
        }
    }
}

impl StreamManifest {
    pub fn host_buffer(&self) -> usize {
        let requested = self.host_buffer.max(1);
        let max = max_host_buffer();
        if requested > max {
            tracing::warn!(requested, max, "host buffer too large; clamping to avoid excessive memory use");
            max
        } else {
            requested
        }
    }

    pub fn preview_jpeg_quality(&self) -> u8 {
        self.preview_jpeg_quality.clamp(1, 100)
    }

    pub fn resolve(&self) -> ResolvedStreamConfig {
        let mut requested = self.clone();
        let host_buffer = requested.host_buffer();

        let pipeline_enabled = requested.pipeline_enabled;
        if !pipeline_enabled {
            requested.pipelines.clear();
            requested.active_pipeline_id = None;
            requested.active_pipeline_output = None;
            requested.pipeline_layout = None;
            requested.pipeline_wires.clear();
        }

        let encoder_explicitly_disabled = requested.encoder.is_disabled();
        if !encoder_explicitly_disabled {
            normalize_stream_encoder_selection(&mut requested);
        }
        let decoder_explicitly_disabled = requested.decoder.is_disabled();
        if !decoder_explicitly_disabled {
            normalize_stream_decoder_selection(&mut requested);
        }

        let mut encoder = ResolvedEncoderConfig {
            enabled: false,
            codec_id: normalized_codec_selector(requested.encoder.id()),
            settings: requested.encoder.settings().cloned(),
            settings_present: false,
        };
        encoder.enabled = !encoder_explicitly_disabled && encoder.codec_id.is_some();

        if encoder.enabled {
            apply_default_encoder_settings(&requested.capture, &mut encoder);
        } else {
            encoder.codec_id = None;
            encoder.settings = None;
        }
        encoder.settings_present = encoder.enabled && encoder.settings.is_some();

        let mut decoder = ResolvedDecoderConfig {
            enabled: false,
            codec_id: normalized_codec_selector(requested.decoder.id()),
            settings: requested.decoder.settings().cloned(),
            settings_present: false,
        };
        decoder.enabled = !decoder_explicitly_disabled && decoder.codec_id.is_some();
        if !decoder.enabled {
            decoder.codec_id = None;
            decoder.settings = None;
        }
        decoder.settings_present = decoder.enabled && decoder.settings.is_some();

        let preview_jpeg_quality = requested.preview_jpeg_quality();

        ResolvedStreamConfig {
            identity: requested.identity,
            capture: requested.capture,
            host_buffer,
            internal: requested.internal,
            pipeline_enabled,
            pipelines: requested.pipelines,
            active_pipeline_id: requested.active_pipeline_id,
            active_pipeline_output: requested.active_pipeline_output,
            pipeline_layout: requested.pipeline_layout,
            pipeline_wires: requested.pipeline_wires,
            pipeline_host_inputs: requested.pipeline_host_inputs,
            calibration: requested.calibration,
            pose: requested.pose,
            encoder,
            decoder,
            preview_jpeg_quality,
            shadow_recorder_enabled: requested.shadow_recorder_enabled,
            start_on_boot: requested.start_on_boot,
        }
    }
}

impl ResolvedStreamConfig {
    pub fn host_buffer(&self) -> usize {
        self.host_buffer
    }

    pub fn to_requested_manifest(&self) -> StreamManifest {
        StreamManifest {
            identity: self.identity.clone(),
            capture: self.capture.clone(),
            host_buffer: self.host_buffer,
            internal: self.internal,
            pipeline_enabled: self.pipeline_enabled,
            pipelines: self.pipelines.clone(),
            active_pipeline_id: self.active_pipeline_id,
            active_pipeline_output: self.active_pipeline_output.clone(),
            pipeline_layout: self.pipeline_layout.clone(),
            pipeline_wires: self.pipeline_wires.clone(),
            pipeline_host_inputs: self.pipeline_host_inputs.clone(),
            calibration: self.calibration.clone(),
            pose: self.pose.clone(),
            encoder: if self.encoder.enabled {
                RequestedEncoderConfig::enabled(self.encoder.codec_id.clone(), self.encoder.settings.clone())
            } else {
                RequestedEncoderConfig::disabled()
            },
            decoder: if self.decoder.enabled {
                RequestedDecoderConfig::enabled(self.decoder.codec_id.clone(), self.decoder.settings.clone())
            } else {
                RequestedDecoderConfig::disabled()
            },
            preview_jpeg_quality: self.preview_jpeg_quality,
            shadow_recorder_enabled: self.shadow_recorder_enabled,
            start_on_boot: self.start_on_boot,
        }
    }

    pub fn encoder_id(&self) -> Option<&str> {
        self.encoder.enabled.then_some(self.encoder.codec_id.as_deref()).flatten()
    }

    pub fn decoder_id(&self) -> Option<&str> {
        self.decoder.enabled.then_some(self.decoder.codec_id.as_deref()).flatten()
    }

    pub fn encoder_settings(&self) -> Option<&EncoderSettings> {
        self.encoder.enabled.then_some(self.encoder.settings.as_ref()).flatten()
    }

    pub fn decoder_settings(&self) -> Option<&DecoderSettings> {
        self.decoder.enabled.then_some(self.decoder.settings.as_ref()).flatten()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamSummary {
    pub stream_id: Uuid,
    pub descriptor: CaptureDescriptor,
    pub manifest: ResolvedStreamConfig,
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

pub fn default_host_buffer() -> usize {
    // The host bridge buffers full frames for late subscribers. At full resolution this can
    // balloon RSS quickly (e.g. RGBA at 2K+). Default small to keep memory predictable; users can
    // still override via `HELIOS_HOST_BUFFER` or per-stream `host_buffer`.
    let requested = env::var("HELIOS_HOST_BUFFER").ok().and_then(|v| v.parse().ok()).filter(|v| *v > 0).unwrap_or(2);
    requested.min(max_host_buffer())
}

const DEFAULT_PREVIEW_JPEG_QUALITY: u8 = 65;
const DEFAULT_STREAM_ENCODER_FPS: u32 = 60;
const DEFAULT_STREAM_ENCODER_OUTPUT_HEIGHT: u32 = 480;
const DEFAULT_STREAM_PREVIEW_JPEG_QUALITY: u8 = 30;

fn default_preview_jpeg_quality_override() -> Option<u8> {
    env::var("HELIOS_PREVIEW_JPEG_QUALITY").ok().and_then(|v| v.parse::<u8>().ok())
}

pub(crate) fn default_shadow_recorder_enabled() -> bool {
    false
}

fn max_host_buffer() -> usize {
    env::var("HELIOS_HOST_BUFFER_MAX").ok().and_then(|v| v.parse().ok()).filter(|v| *v > 0).unwrap_or(64)
}

fn manifest_prefers_default_stream_encoder(manifest: &StreamManifest) -> bool {
    !manifest.internal && !matches!(manifest.capture.backend, BackendKind::File | BackendKind::Netcam)
}

fn encoder_selector_needs_normalization(selector: Option<&str>) -> bool {
    let Some(selector) = selector.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    selector.eq_ignore_ascii_case("ffmpeg") || matches!(selector.to_ascii_lowercase().as_str(), "mjpeg" | "mjpg" | "jpeg")
}

pub fn default_stream_encoder_selector() -> Option<String> {
    let preferred_input = FourCc::new(*b"RG24");
    let entries = CodecRegistry::list_enabled_encoders().ok()?;
    let mut preferred_mjpeg: Option<String> = None;
    let mut fallback_mjpeg: Option<String> = None;
    let mut fallback_any: Option<String> = None;

    for (input, codecs) in entries {
        if input != preferred_input {
            continue;
        }
        for desc in codecs {
            if desc.kind != CodecKind::Encoder {
                continue;
            }
            let impl_name = desc.impl_name.trim();
            if impl_name.is_empty() {
                continue;
            }
            if fallback_any.is_none() {
                fallback_any = Some(impl_name.to_string());
            }
            if desc.name.eq_ignore_ascii_case("mjpeg") {
                if desc.impl_name.eq_ignore_ascii_case("turbojpeg") {
                    preferred_mjpeg = Some(impl_name.to_string());
                    break;
                }
                if fallback_mjpeg.is_none() {
                    fallback_mjpeg = Some(impl_name.to_string());
                }
            }
        }
        if preferred_mjpeg.is_some() {
            break;
        }
    }

    preferred_mjpeg.or(fallback_mjpeg).or(fallback_any)
}

pub fn normalize_requested_stream_encoder(manifest: &mut StreamManifest) {
    normalize_stream_encoder_selection(manifest);
}

pub fn normalize_requested_stream_decoder(manifest: &mut StreamManifest) {
    normalize_stream_decoder_selection(manifest);
}

fn normalize_stream_encoder_selection(manifest: &mut StreamManifest) {
    if !manifest_prefers_default_stream_encoder(manifest) {
        return;
    }

    if manifest.encoder.is_disabled() {
        return;
    }

    let selector_needs_normalization = encoder_selector_needs_normalization(manifest.encoder.id());
    if selector_needs_normalization {
        let Some(default_selector) = default_stream_encoder_selector() else {
            return;
        };
        manifest.encoder.ensure_id(Some(default_selector));
    }
}

fn normalize_stream_decoder_selection(manifest: &mut StreamManifest) {
    if manifest.decoder.is_disabled() {
        return;
    }

    let Some(default_selector) = default_decoder_selector_for_capture_format(manifest.capture.mode.format.code) else {
        return;
    };
    manifest.decoder.ensure_id(Some(default_selector));
}

fn normalized_codec_selector(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(ToString::to_string)
}

fn default_decoder_selector_for_codec(descs: &[styx::codec::CodecDescriptor]) -> Option<String> {
    if descs.is_empty() {
        return None;
    }

    if let Some(codec) = descs.iter().find(|desc| desc.name.eq_ignore_ascii_case("mjpeg") && desc.impl_name.eq_ignore_ascii_case("turbojpeg")) {
        return Some(codec.impl_name.to_string());
    }

    match descs[0].input.to_u32().to_le_bytes() {
        [b'H', b'2', b'6', b'4'] => return Some("h264".to_string()),
        [b'H', b'2', b'6', b'5'] | [b'H', b'E', b'V', b'C'] => return Some("h265".to_string()),
        [b'M', b'J', b'P', b'G'] | [b'J', b'P', b'E', b'G'] => {}
        _ => {}
    }

    if let Some(codec) = descs.iter().find(|desc| desc.impl_name.eq_ignore_ascii_case("passthrough")) {
        return Some(codec.impl_name.to_string());
    }

    descs.first().map(|desc| desc.impl_name.to_string())
}

pub fn default_decoder_ids_by_capture_format() -> BTreeMap<String, String> {
    let mut defaults = BTreeMap::new();
    let Ok(entries) = CodecRegistry::list_enabled_codecs() else {
        return defaults;
    };

    for (input, codecs) in entries {
        let decoder_descs: Vec<_> = codecs.into_iter().filter(|desc| desc.kind == CodecKind::Decoder).collect();
        if decoder_descs.is_empty() {
            continue;
        }
        let key = String::from_utf8_lossy(&input.to_u32().to_le_bytes()).trim().to_ascii_uppercase();
        if key.is_empty() {
            continue;
        }
        if let Some(selector) = default_decoder_selector_for_codec(&decoder_descs) {
            defaults.insert(key, selector);
        }
    }

    defaults
}

pub fn default_decoder_selector_for_capture_format(fourcc: FourCc) -> Option<String> {
    let defaults = default_decoder_ids_by_capture_format();
    let key = String::from_utf8_lossy(&fourcc.to_u32().to_le_bytes()).trim().to_ascii_uppercase();
    defaults.get(&key).cloned().or_else(|| defaults.get("ANY").cloned())
}

fn default_encoder_output_resolution(capture_resolution: Resolution) -> ResolutionHint {
    let source_width = capture_resolution.width.get().max(1);
    let source_height = capture_resolution.height.get().max(1);
    let target_height = source_height.min(DEFAULT_STREAM_ENCODER_OUTPUT_HEIGHT).max(1);

    if source_height <= target_height {
        return ResolutionHint { width: source_width, height: source_height };
    }

    let scale = target_height as f64 / source_height as f64;
    let mut width = ((source_width as f64) * scale).round() as u32;
    let mut height = target_height;

    if width > 1 && width % 2 != 0 {
        width += 1;
    }
    if height > 1 && height % 2 != 0 {
        height -= 1;
    }

    ResolutionHint { width: width.max(1).min(source_width), height: height.max(1).min(source_height) }
}

fn apply_default_encoder_settings(capture: &CaptureConfig, encoder: &mut ResolvedEncoderConfig) {
    let settings = encoder.settings.get_or_insert_with(Default::default);
    if settings.framerate.is_none() {
        settings.framerate = Some(FrameRate { numerator: DEFAULT_STREAM_ENCODER_FPS, denominator: 1 });
    }
    let has_explicit_resolution = settings.output_resolution.as_ref().is_some_and(|resolution| resolution.width > 0 && resolution.height > 0);
    if !has_explicit_resolution {
        settings.output_resolution = Some(default_encoder_output_resolution(capture.mode.format.resolution));
    }
}
