use lib_cv::modules::calibration::LensModel;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::env;
use std::sync::OnceLock;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::capture::{CaptureConfig, CaptureControlInfo, CaptureControlValue, CaptureDescriptor};
use crate::contracts::stream_ids::RAW_PIPELINE_UUID;
use crate::identity::DeviceIdentity;
use crate::stream::{StreamEncoderDemandMetrics, StreamFrameDemandMetrics, StreamMetrics};

use bincode::error::{DecodeError, EncodeError};
use bincode::{Decode, Encode};
use lib_ipc::frame::MessageKind;
use lib_ipc::protocol::ControlEvent;
use lib_ipc::server::ServerEvent;
use lib_ipc::types::CommandId;
use styx::codec::{CodecKind, CodecRegistry};
use styx::prelude::{FourCc, Resolution};
use styx::BackendKind;

mod generated_codec_families;

use self::generated_codec_families::{GeneratedEncoderFamilySpec, GeneratedEncoderFamilyVariant, GENERATED_ENCODER_FAMILY_SPECS};

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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct StreamCodecTunables {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_settings: Option<EncoderSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct StreamCodecCapability {
    pub kind: CodecKind,
    pub fourcc: String,
    pub name: String,
    pub implementation: String,
    pub input: String,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunables: Option<StreamCodecTunables>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StreamRuntimeCapabilities {
    #[serde(default)]
    pub codecs: Vec<StreamCodecCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_encoder_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub default_decoder_ids_by_capture_format: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum EngineCommand {
    List {
        #[bincode(with_serde)]
        command_id: CommandId,
    },
    GetStreamRuntimeCapabilities {
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

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, ToSchema, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, ToSchema, PartialEq)]
pub struct ResolutionHint {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EncoderSettingsKind {
    Turbojpeg,
    Mozjpeg,
    FfmpegMjpeg,
    H264,
    H265,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode, ToSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EncoderSettings {
    Turbojpeg {
        #[serde(default)]
        quality: Option<u8>,
    },
    Mozjpeg {
        #[serde(default)]
        quality: Option<u8>,
    },
    FfmpegMjpeg {
        #[serde(default)]
        bitrate: Option<u64>,
        #[serde(default)]
        gop: Option<i32>,
        #[serde(default, deserialize_with = "deserialize_encoder_frame_rate")]
        framerate: Option<FrameRate>,
        #[serde(default)]
        thread_count: Option<usize>,
        #[serde(default)]
        output_resolution: Option<ResolutionHint>,
    },
    H264 {
        #[serde(default)]
        bitrate: Option<u64>,
        #[serde(default)]
        gop: Option<i32>,
        #[serde(default, deserialize_with = "deserialize_encoder_frame_rate")]
        framerate: Option<FrameRate>,
        #[serde(default)]
        thread_count: Option<usize>,
        #[serde(default)]
        output_resolution: Option<ResolutionHint>,
    },
    H265 {
        #[serde(default)]
        bitrate: Option<u64>,
        #[serde(default)]
        gop: Option<i32>,
        #[serde(default, deserialize_with = "deserialize_encoder_frame_rate")]
        framerate: Option<FrameRate>,
        #[serde(default)]
        thread_count: Option<usize>,
        #[serde(default)]
        output_resolution: Option<ResolutionHint>,
    },
}

impl EncoderSettings {
    fn settings_kind(&self) -> EncoderSettingsKind {
        match self {
            Self::Turbojpeg { .. } => EncoderSettingsKind::Turbojpeg,
            Self::Mozjpeg { .. } => EncoderSettingsKind::Mozjpeg,
            Self::FfmpegMjpeg { .. } => EncoderSettingsKind::FfmpegMjpeg,
            Self::H264 { .. } => EncoderSettingsKind::H264,
            Self::H265 { .. } => EncoderSettingsKind::H265,
        }
    }

    pub fn bitrate(&self) -> Option<u64> {
        match self {
            Self::Turbojpeg { .. } | Self::Mozjpeg { .. } => None,
            Self::FfmpegMjpeg { bitrate, .. } | Self::H264 { bitrate, .. } | Self::H265 { bitrate, .. } => *bitrate,
        }
    }

    pub fn gop(&self) -> Option<i32> {
        match self {
            Self::Turbojpeg { .. } | Self::Mozjpeg { .. } => None,
            Self::FfmpegMjpeg { gop, .. } | Self::H264 { gop, .. } | Self::H265 { gop, .. } => *gop,
        }
    }

    pub fn framerate(&self) -> Option<&FrameRate> {
        match self {
            Self::Turbojpeg { .. } | Self::Mozjpeg { .. } => None,
            Self::FfmpegMjpeg { framerate, .. } | Self::H264 { framerate, .. } | Self::H265 { framerate, .. } => framerate.as_ref(),
        }
    }

    pub fn thread_count(&self) -> Option<usize> {
        match self {
            Self::Turbojpeg { .. } | Self::Mozjpeg { .. } => None,
            Self::FfmpegMjpeg { thread_count, .. } | Self::H264 { thread_count, .. } | Self::H265 { thread_count, .. } => *thread_count,
        }
    }

    pub fn output_resolution(&self) -> Option<&ResolutionHint> {
        match self {
            Self::Turbojpeg { .. } | Self::Mozjpeg { .. } => None,
            Self::FfmpegMjpeg { output_resolution, .. } | Self::H264 { output_resolution, .. } | Self::H265 { output_resolution, .. } => output_resolution.as_ref(),
        }
    }

    pub fn quality(&self) -> Option<u8> {
        match self {
            Self::Turbojpeg { quality } | Self::Mozjpeg { quality } => *quality,
            Self::FfmpegMjpeg { .. } | Self::H264 { .. } | Self::H265 { .. } => None,
        }
    }

    fn ensure_video_framerate(&mut self, default_framerate: FrameRate) {
        match self {
            Self::Turbojpeg { .. } | Self::Mozjpeg { .. } => {}
            Self::FfmpegMjpeg { framerate, .. } | Self::H264 { framerate, .. } | Self::H265 { framerate, .. } => {
                if framerate.is_none() {
                    *framerate = Some(default_framerate);
                }
            }
        }
    }

    fn ensure_video_output_resolution(&mut self, default_resolution: ResolutionHint) {
        match self {
            Self::Turbojpeg { .. } | Self::Mozjpeg { .. } => {}
            Self::FfmpegMjpeg { output_resolution, .. } | Self::H264 { output_resolution, .. } | Self::H265 { output_resolution, .. } => {
                let has_explicit_resolution = output_resolution.as_ref().is_some_and(|resolution| resolution.width > 0 && resolution.height > 0);
                if !has_explicit_resolution {
                    *output_resolution = Some(default_resolution);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum EncoderSettingsBinaryWire {
    Turbojpeg {
        #[serde(default)]
        quality: Option<u8>,
    },
    Mozjpeg {
        #[serde(default)]
        quality: Option<u8>,
    },
    FfmpegMjpeg {
        #[serde(default)]
        bitrate: Option<u64>,
        #[serde(default)]
        gop: Option<i32>,
        #[serde(default)]
        framerate: Option<FrameRate>,
        #[serde(default)]
        thread_count: Option<usize>,
        #[serde(default)]
        output_resolution: Option<ResolutionHint>,
    },
    H264 {
        #[serde(default)]
        bitrate: Option<u64>,
        #[serde(default)]
        gop: Option<i32>,
        #[serde(default)]
        framerate: Option<FrameRate>,
        #[serde(default)]
        thread_count: Option<usize>,
        #[serde(default)]
        output_resolution: Option<ResolutionHint>,
    },
    H265 {
        #[serde(default)]
        bitrate: Option<u64>,
        #[serde(default)]
        gop: Option<i32>,
        #[serde(default)]
        framerate: Option<FrameRate>,
        #[serde(default)]
        thread_count: Option<usize>,
        #[serde(default)]
        output_resolution: Option<ResolutionHint>,
    },
}

impl From<EncoderSettings> for EncoderSettingsBinaryWire {
    fn from(value: EncoderSettings) -> Self {
        match value {
            EncoderSettings::Turbojpeg { quality } => Self::Turbojpeg { quality },
            EncoderSettings::Mozjpeg { quality } => Self::Mozjpeg { quality },
            EncoderSettings::FfmpegMjpeg { bitrate, gop, framerate, thread_count, output_resolution } => Self::FfmpegMjpeg { bitrate, gop, framerate, thread_count, output_resolution },
            EncoderSettings::H264 { bitrate, gop, framerate, thread_count, output_resolution } => Self::H264 { bitrate, gop, framerate, thread_count, output_resolution },
            EncoderSettings::H265 { bitrate, gop, framerate, thread_count, output_resolution } => Self::H265 { bitrate, gop, framerate, thread_count, output_resolution },
        }
    }
}

impl From<EncoderSettingsBinaryWire> for EncoderSettings {
    fn from(value: EncoderSettingsBinaryWire) -> Self {
        match value {
            EncoderSettingsBinaryWire::Turbojpeg { quality } => Self::Turbojpeg { quality },
            EncoderSettingsBinaryWire::Mozjpeg { quality } => Self::Mozjpeg { quality },
            EncoderSettingsBinaryWire::FfmpegMjpeg { bitrate, gop, framerate, thread_count, output_resolution } => Self::FfmpegMjpeg { bitrate, gop, framerate, thread_count, output_resolution },
            EncoderSettingsBinaryWire::H264 { bitrate, gop, framerate, thread_count, output_resolution } => Self::H264 { bitrate, gop, framerate, thread_count, output_resolution },
            EncoderSettingsBinaryWire::H265 { bitrate, gop, framerate, thread_count, output_resolution } => Self::H265 { bitrate, gop, framerate, thread_count, output_resolution },
        }
    }
}

fn deserialize_encoder_frame_rate<'de, D>(deserializer: D) -> Result<Option<FrameRate>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<FrameRate>::deserialize(deserializer)
}

fn encoder_settings_kind_from_generated_variant(variant: GeneratedEncoderFamilyVariant) -> EncoderSettingsKind {
    match variant {
        GeneratedEncoderFamilyVariant::Turbojpeg => EncoderSettingsKind::Turbojpeg,
        GeneratedEncoderFamilyVariant::Mozjpeg => EncoderSettingsKind::Mozjpeg,
        GeneratedEncoderFamilyVariant::FfmpegMjpeg => EncoderSettingsKind::FfmpegMjpeg,
        GeneratedEncoderFamilyVariant::H264 => EncoderSettingsKind::H264,
        GeneratedEncoderFamilyVariant::H265 => EncoderSettingsKind::H265,
    }
}

fn generated_encoder_family_spec_for_kind(kind: EncoderSettingsKind) -> &'static GeneratedEncoderFamilySpec {
    GENERATED_ENCODER_FAMILY_SPECS.iter().find(|spec| encoder_settings_kind_from_generated_variant(spec.variant) == kind).expect("generated encoder family spec for settings kind")
}

fn generated_encoder_family_spec_for_runtime_descriptor(desc: &styx::codec::CodecDescriptor) -> Option<&'static GeneratedEncoderFamilySpec> {
    if desc.kind != CodecKind::Encoder {
        return None;
    }

    GENERATED_ENCODER_FAMILY_SPECS.iter().find(|spec| {
        let implementation_matches = spec.runtime_implementation_aliases.iter().any(|alias| desc.impl_name.eq_ignore_ascii_case(alias));
        if !implementation_matches {
            return false;
        }
        if !desc.impl_name.eq_ignore_ascii_case("ffmpeg") {
            return true;
        }

        let output = desc.output.to_string();
        spec.runtime_name_aliases.iter().any(|alias| desc.name.eq_ignore_ascii_case(alias)) || spec.output_fourcc_aliases.iter().any(|alias| output.eq_ignore_ascii_case(alias))
    })
}

fn generated_encoder_family_spec_for_runtime_codec(implementation: &str, fourcc: FourCc) -> Option<&'static GeneratedEncoderFamilySpec> {
    GENERATED_ENCODER_FAMILY_SPECS.iter().find(|spec| {
        let implementation_matches = spec.runtime_implementation_aliases.iter().any(|alias| implementation.eq_ignore_ascii_case(alias));
        if !implementation_matches {
            return false;
        }
        if !implementation.eq_ignore_ascii_case("ffmpeg") {
            return true;
        }

        let output = fourcc.to_string();
        spec.output_fourcc_aliases.iter().any(|alias| output.eq_ignore_ascii_case(alias))
    })
}

fn generated_encoder_family_spec_for_selector_alias(selector: &str) -> Option<&'static GeneratedEncoderFamilySpec> {
    GENERATED_ENCODER_FAMILY_SPECS.iter().find(|spec| {
        spec.selector_id.eq_ignore_ascii_case(selector)
            || spec.selector_aliases.iter().any(|alias| selector.eq_ignore_ascii_case(alias))
            || (!selector.eq_ignore_ascii_case("ffmpeg") && spec.runtime_implementation_aliases.iter().any(|alias| selector.eq_ignore_ascii_case(alias)))
    })
}

fn encoder_settings_kind_for_codec_desc(desc: &styx::codec::CodecDescriptor) -> Option<EncoderSettingsKind> {
    generated_encoder_family_spec_for_runtime_descriptor(desc).map(|spec| encoder_settings_kind_from_generated_variant(spec.variant))
}

fn encoder_settings_kind_for_selector(selector: Option<&str>) -> Option<EncoderSettingsKind> {
    let selector = selector.map(str::trim).filter(|value| !value.is_empty())?;
    if let Some(spec) = generated_encoder_family_spec_for_selector_alias(selector) {
        return Some(encoder_settings_kind_from_generated_variant(spec.variant));
    }

    let Ok(entries) = CodecRegistry::list_enabled_codecs() else {
        return None;
    };
    let mut matches = std::collections::BTreeSet::new();
    for (_, codecs) in entries {
        for desc in codecs {
            if desc.kind != CodecKind::Encoder {
                continue;
            }
            if !(desc.impl_name.eq_ignore_ascii_case(selector) || desc.name.eq_ignore_ascii_case(selector)) {
                continue;
            }
            if let Some(kind) = encoder_settings_kind_for_codec_desc(&desc) {
                matches.insert(kind);
            }
        }
    }

    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

fn selector_name_for_encoder_settings_kind(kind: EncoderSettingsKind) -> &'static str {
    generated_encoder_family_spec_for_kind(kind).selector_id
}

pub fn default_encoder_settings_for_codec(fourcc: FourCc, implementation: &str) -> Option<EncoderSettings> {
    let spec = generated_encoder_family_spec_for_runtime_codec(implementation, fourcc)?;
    match spec.variant {
        GeneratedEncoderFamilyVariant::Turbojpeg => Some(EncoderSettings::Turbojpeg { quality: Some(85) }),
        GeneratedEncoderFamilyVariant::Mozjpeg => Some(EncoderSettings::Mozjpeg { quality: Some(85) }),
        GeneratedEncoderFamilyVariant::FfmpegMjpeg | GeneratedEncoderFamilyVariant::H264 | GeneratedEncoderFamilyVariant::H265 => {
            let default_framerate = Some(FrameRate { numerator: 60, denominator: 1 });
            let default_output_resolution = Some(ResolutionHint { width: 854, height: 480 });
            match spec.variant {
                GeneratedEncoderFamilyVariant::FfmpegMjpeg => {
                    Some(EncoderSettings::FfmpegMjpeg { bitrate: Some(4_000_000), gop: None, framerate: default_framerate, thread_count: None, output_resolution: default_output_resolution })
                }
                GeneratedEncoderFamilyVariant::H264 => {
                    Some(EncoderSettings::H264 { bitrate: Some(4_000_000), gop: None, framerate: default_framerate, thread_count: None, output_resolution: default_output_resolution })
                }
                GeneratedEncoderFamilyVariant::H265 => {
                    Some(EncoderSettings::H265 { bitrate: Some(4_000_000), gop: None, framerate: default_framerate, thread_count: None, output_resolution: default_output_resolution })
                }
                GeneratedEncoderFamilyVariant::Turbojpeg | GeneratedEncoderFamilyVariant::Mozjpeg => unreachable!("handled above"),
            }
        }
    }
}

pub fn empty_encoder_settings_for_selector(selector: Option<&str>) -> Option<EncoderSettings> {
    match encoder_settings_kind_for_selector(selector) {
        Some(EncoderSettingsKind::Turbojpeg) => Some(EncoderSettings::Turbojpeg { quality: None }),
        Some(EncoderSettingsKind::Mozjpeg) => Some(EncoderSettings::Mozjpeg { quality: None }),
        Some(EncoderSettingsKind::FfmpegMjpeg) => Some(EncoderSettings::FfmpegMjpeg { bitrate: None, gop: None, framerate: None, thread_count: None, output_resolution: None }),
        Some(EncoderSettingsKind::H264) => Some(EncoderSettings::H264 { bitrate: None, gop: None, framerate: None, thread_count: None, output_resolution: None }),
        Some(EncoderSettingsKind::H265) => Some(EncoderSettings::H265 { bitrate: None, gop: None, framerate: None, thread_count: None, output_resolution: None }),
        None => None,
    }
}

fn canonical_encoder_selector(selector: Option<&str>, settings: Option<&EncoderSettings>) -> Option<String> {
    let selector = selector.map(str::trim).filter(|value| !value.is_empty())?;
    if selector.eq_ignore_ascii_case("ffmpeg") {
        return settings.map(EncoderSettings::settings_kind).map(selector_name_for_encoder_settings_kind).map(ToString::to_string);
    }
    if let Some(spec) =
        GENERATED_ENCODER_FAMILY_SPECS.iter().find(|spec| !spec.selector_id.eq_ignore_ascii_case(selector) && spec.selector_aliases.iter().any(|alias| selector.eq_ignore_ascii_case(alias)))
    {
        return Some(spec.selector_id.to_string());
    }
    None
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
        retryable: bool,
    },
    StreamList {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        streams: Vec<StreamSummary>,
    },
    StreamRuntimeCapabilities {
        #[bincode(with_serde)]
        command_id: CommandId,
        #[bincode(with_serde)]
        capabilities: StreamRuntimeCapabilities,
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
            | Self::StreamRuntimeCapabilities { .. }
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
            ControlEvent::Nack(nack) => EngineEvent::Nack { command_id: nack.command_id, code: EngineErrorCode::InvalidState, reason: nack.reason, retryable: nack.retryable },
        }
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode, ToSchema, PartialEq, Eq, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StreamRecordingMode {
    #[default]
    Disabled,
    ShadowBuffer {
        codec: RecordingCodec,
    },
}

impl StreamRecordingMode {
    pub fn disabled() -> Self {
        Self::Disabled
    }

    pub fn shadow_buffer(codec: RecordingCodec) -> Self {
        Self::ShadowBuffer { codec }
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    pub fn is_shadow_buffer(&self) -> bool {
        matches!(self, Self::ShadowBuffer { .. })
    }

    pub fn shadow_buffer_codec(&self) -> Option<RecordingCodec> {
        match self {
            Self::Disabled => None,
            Self::ShadowBuffer { codec } => Some(*codec),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum StreamRecordingModeHumanWire {
    Disabled,
    ShadowBuffer { codec: RecordingCodec },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum StreamRecordingModeBinaryWire {
    Disabled,
    ShadowBuffer { codec: RecordingCodec },
}

impl From<StreamRecordingMode> for StreamRecordingModeHumanWire {
    fn from(value: StreamRecordingMode) -> Self {
        match value {
            StreamRecordingMode::Disabled => Self::Disabled,
            StreamRecordingMode::ShadowBuffer { codec } => Self::ShadowBuffer { codec },
        }
    }
}

impl From<StreamRecordingModeHumanWire> for StreamRecordingMode {
    fn from(value: StreamRecordingModeHumanWire) -> Self {
        match value {
            StreamRecordingModeHumanWire::Disabled => Self::Disabled,
            StreamRecordingModeHumanWire::ShadowBuffer { codec } => Self::ShadowBuffer { codec },
        }
    }
}

impl From<StreamRecordingMode> for StreamRecordingModeBinaryWire {
    fn from(value: StreamRecordingMode) -> Self {
        match value {
            StreamRecordingMode::Disabled => Self::Disabled,
            StreamRecordingMode::ShadowBuffer { codec } => Self::ShadowBuffer { codec },
        }
    }
}

impl From<StreamRecordingModeBinaryWire> for StreamRecordingMode {
    fn from(value: StreamRecordingModeBinaryWire) -> Self {
        match value {
            StreamRecordingModeBinaryWire::Disabled => Self::Disabled,
            StreamRecordingModeBinaryWire::ShadowBuffer { codec } => Self::ShadowBuffer { codec },
        }
    }
}

mod stream_recording_mode_serde {
    use super::{StreamRecordingMode, StreamRecordingModeBinaryWire, StreamRecordingModeHumanWire};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &StreamRecordingMode, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            StreamRecordingModeHumanWire::from(*value).serialize(serializer)
        } else {
            StreamRecordingModeBinaryWire::from(*value).serialize(serializer)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<StreamRecordingMode, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            Ok(StreamRecordingModeHumanWire::deserialize(deserializer)?.into())
        } else {
            Ok(StreamRecordingModeBinaryWire::deserialize(deserializer)?.into())
        }
    }
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
    #[serde(default, with = "stream_recording_mode_serde")]
    pub recording_mode: StreamRecordingMode,
    pub start_on_boot: bool,
}

#[derive(Debug, Clone)]
pub enum RequestedEncoderConfig {
    Disabled,
    Enabled { id: Option<String>, settings: Option<EncoderSettings> },
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
        settings: Option<EncoderSettingsBinaryWire>,
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

    pub fn set_id(&mut self, id: Option<String>) {
        match self {
            Self::Disabled => {}
            Self::Enabled { id: existing, .. } => {
                *existing = normalized_codec_selector(id.as_deref());
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
}

impl From<RequestedEncoderConfig> for RequestedEncoderConfigBinaryWire {
    fn from(value: RequestedEncoderConfig) -> Self {
        match value {
            RequestedEncoderConfig::Disabled => Self::Disabled,
            RequestedEncoderConfig::Enabled { id, settings } => Self::Enabled { id, settings: settings.map(EncoderSettingsBinaryWire::from) },
        }
    }
}

impl From<RequestedEncoderConfigBinaryWire> for RequestedEncoderConfig {
    fn from(value: RequestedEncoderConfigBinaryWire) -> Self {
        match value {
            RequestedEncoderConfigBinaryWire::Disabled => Self::Disabled,
            RequestedEncoderConfigBinaryWire::Enabled { id, settings } => Self::Enabled { id, settings: settings.map(EncoderSettings::from) },
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

impl RequestedEncoderConfigHumanWire {
    fn into_requested(self) -> Result<(RequestedEncoderConfig, Option<f64>), String> {
        match self {
            Self::Disabled => Ok((RequestedEncoderConfig::Disabled, None)),
            Self::Enabled { id, settings } => {
                let normalized_id = normalized_codec_selector(id.as_deref());
                Ok((RequestedEncoderConfig::Enabled { id: normalized_id, settings }, None))
            }
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
            RequestedEncoderConfigHumanWire::deserialize(deserializer)?.into_requested().map(|(config, _)| config).map_err(serde::de::Error::custom)
        } else {
            Ok(RequestedEncoderConfigBinaryWire::deserialize(deserializer)?.into())
        }
    }
}

#[derive(Debug, Clone)]
pub enum RequestedDecoderConfig {
    Disabled,
    Enabled { id: Option<String>, settings: Option<DecoderSettings> },
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

    pub fn ensure_fps_limit(&mut self, fps_limit: Option<f64>) {
        let fps_limit = fps_limit.filter(|value| value.is_finite() && *value > 0.0);
        let Some(fps_limit) = fps_limit else {
            return;
        };
        match self {
            Self::Disabled => {}
            Self::Enabled { settings, .. } => {
                let settings = settings.get_or_insert_with(Default::default);
                if settings.fps_limit.is_none() {
                    settings.fps_limit = Some(fps_limit);
                }
            }
        }
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
    if value == 0 {
        default_host_buffer()
    } else {
        value
    }
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
        !pipelines.is_empty() || active_pipeline_id.is_some() || active_pipeline_output.is_some_and(|value| !value.trim().is_empty()) || pipeline_layout.is_some() || !pipeline_wires.is_empty()
    })
}

fn default_requested_preview_jpeg_quality(encoder: &RequestedEncoderConfig) -> u8 {
    if encoder.is_disabled() {
        default_requested_preview_jpeg_quality_disabled()
    } else {
        default_requested_preview_jpeg_quality_enabled()
    }
}

fn canonical_requested_preview_jpeg_quality(value: Option<u8>, encoder: &RequestedEncoderConfig) -> u8 {
    value.map(|value| value.clamp(1, 100)).unwrap_or_else(|| default_requested_preview_jpeg_quality(encoder))
}

pub const CURRENT_STREAM_CONFIG_SCHEMA_VERSION: u32 = 1;

fn trim_pipeline_output_selection(value: Option<&str>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub fn normalize_pipeline_output_selection(value: Option<&str>, pipeline_id: Option<Uuid>) -> Result<Option<String>, String> {
    let normalized = trim_pipeline_output_selection(value);
    if pipeline_id != Some(RAW_PIPELINE_UUID) {
        return Ok(normalized);
    }

    match normalized.as_deref() {
        None => Ok(None),
        Some(raw) if raw.eq_ignore_ascii_case("raw") => Ok(Some("raw".to_string())),
        Some(raw) if raw.eq_ignore_ascii_case("undistorted") => Ok(Some("undistorted".to_string())),
        Some(raw) => Err(format!("unsupported RAW pipeline output '{raw}'; expected `raw` or `undistorted`")),
    }
}

#[derive(Debug, Clone, ToSchema)]
pub struct StreamManifest {
    pub schema_version: u32,
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
    /// Recording mode contract for capture-last buffers and future recording policies.
    pub recording_mode: StreamRecordingMode,
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
    #[serde(default = "default_recording_mode", with = "stream_recording_mode_serde")]
    pub recording_mode: StreamRecordingMode,
    #[serde(default)]
    pub start_on_boot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamManifestHumanWire {
    #[serde(default)]
    pub schema_version: Option<u32>,
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
    pub encoder: Option<RequestedEncoderConfigHumanWire>,
    #[serde(default)]
    pub decoder: Option<RequestedDecoderConfig>,
    #[serde(default)]
    pub preview_jpeg_quality: Option<u8>,
    #[serde(default)]
    pub recording_mode: Option<StreamRecordingMode>,
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
            schema_version: CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
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
            recording_mode: value.recording_mode,
            start_on_boot: value.start_on_boot,
        }
    }
}

fn enforce_current_stream_manifest_schema(mut wire: StreamManifestHumanWire) -> Result<StreamManifestHumanWire, String> {
    match wire.schema_version {
        Some(version) if version == CURRENT_STREAM_CONFIG_SCHEMA_VERSION => {
            wire.schema_version = Some(CURRENT_STREAM_CONFIG_SCHEMA_VERSION);
            Ok(wire)
        }
        Some(version) if version > CURRENT_STREAM_CONFIG_SCHEMA_VERSION => {
            Err(format!("unsupported stream manifest schema_version {}; current version is {}", version, CURRENT_STREAM_CONFIG_SCHEMA_VERSION))
        }
        Some(version) => Err(format!("unsupported stream manifest schema_version {}; old stream manifests are no longer supported after the teardown reset", version)),
        None => Err(format!("stream manifest schema_version is required; legacy manifests without schema_version are no longer supported (expected {})", CURRENT_STREAM_CONFIG_SCHEMA_VERSION)),
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
            recording_mode: value.recording_mode,
            start_on_boot: value.start_on_boot,
        }
    }
}

impl TryFrom<StreamManifestHumanWire> for StreamManifest {
    type Error = String;

    fn try_from(value: StreamManifestHumanWire) -> Result<Self, Self::Error> {
        let value = enforce_current_stream_manifest_schema(value)?;
        let (encoder, _) = match value.encoder {
            Some(encoder) => encoder.into_requested()?,
            None => (RequestedEncoderConfig::default(), None),
        };
        let decoder = value.decoder.unwrap_or_default();
        let recording_mode = value.recording_mode.unwrap_or_else(default_recording_mode);
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
            schema_version: CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
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
            recording_mode,
            start_on_boot: value.start_on_boot,
        })
    }
}

impl From<StreamManifest> for StreamManifestHumanWire {
    fn from(value: StreamManifest) -> Self {
        Self {
            schema_version: Some(value.schema_version),
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
            encoder: Some(value.encoder.into()),
            decoder: Some(value.decoder),
            preview_jpeg_quality: Some(value.preview_jpeg_quality),
            recording_mode: Some(value.recording_mode),
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

        let mut encoder =
            ResolvedEncoderConfig { enabled: false, codec_id: normalized_codec_selector(requested.encoder.id()), settings: requested.encoder.settings().cloned(), settings_present: false };
        encoder.enabled = !encoder_explicitly_disabled && encoder.codec_id.is_some();

        if encoder.enabled {
            apply_default_encoder_settings(&requested.capture, &mut encoder);
        } else {
            encoder.codec_id = None;
            encoder.settings = None;
        }
        encoder.settings_present = encoder.enabled && encoder.settings.is_some();

        let mut decoder =
            ResolvedDecoderConfig { enabled: false, codec_id: normalized_codec_selector(requested.decoder.id()), settings: requested.decoder.settings().cloned(), settings_present: false };
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
            recording_mode: requested.recording_mode,
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
            schema_version: CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
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
            encoder: if self.encoder.enabled { RequestedEncoderConfig::enabled(self.encoder.codec_id.clone(), self.encoder.settings.clone()) } else { RequestedEncoderConfig::disabled() },
            decoder: if self.decoder.enabled { RequestedDecoderConfig::enabled(self.decoder.codec_id.clone(), self.decoder.settings.clone()) } else { RequestedDecoderConfig::disabled() },
            preview_jpeg_quality: self.preview_jpeg_quality,
            recording_mode: self.recording_mode,
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
    #[serde(default)]
    pub runtime: StreamRuntimeState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamRuntimeState {
    #[serde(default)]
    pub capture: StreamCaptureRuntimeState,
    #[serde(default)]
    pub codecs: StreamCodecChainRuntimeState,
    #[serde(default)]
    pub demand: StreamDemandRuntimeState,
    #[serde(default)]
    pub recording: StreamRecordingRuntimeState,
    #[serde(default)]
    pub pipeline: StreamPipelineRuntimeState,
}

impl StreamRuntimeState {
    pub fn status(&self) -> StreamStatus {
        let mut status = match self.capture.state {
            StreamCaptureState::Disabled => StreamStatus {
                state: StreamState::Disabled,
                started_at_ms: self.capture.started_at_ms,
                disabled_since_ms: self.capture.disabled_since_ms,
                disabled_reason: self.capture.disabled_reason.clone(),
                recording_active: false,
                recording_since_ms: None,
            },
            StreamCaptureState::Running | StreamCaptureState::Stopped => StreamStatus { state: StreamState::Running, started_at_ms: self.capture.started_at_ms, ..StreamStatus::default() },
        };
        if self.recording.state == StreamRecordingState::Active {
            status.recording_active = true;
            status.recording_since_ms = self.recording.started_at_ms;
        }
        status
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum StreamCaptureState {
    Running,
    #[default]
    Stopped,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamCaptureRuntimeState {
    #[serde(default)]
    pub state: StreamCaptureState,
    #[serde(default)]
    pub started_at_ms: Option<u64>,
    #[serde(default)]
    pub capture_fourcc: Option<String>,
    #[serde(default)]
    pub disabled_since_ms: Option<u64>,
    #[serde(default)]
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamCodecChainRuntimeState {
    #[serde(default)]
    pub capture_input_fourcc: Option<String>,
    #[serde(default)]
    pub decoder_impl: Option<String>,
    #[serde(default)]
    pub encoder_input_fourcc: Option<String>,
    #[serde(default)]
    pub encoder_impl: Option<String>,
    #[serde(default)]
    pub encoder_output_fourcc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamViewerDemandRuntimeState {
    #[serde(default)]
    pub raw_receiver_count: u64,
    #[serde(default)]
    pub host_receiver_count: u64,
    #[serde(default)]
    pub preview_viewer_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamGraphDemandRuntimeState {
    #[serde(default)]
    pub output_sample_pending: bool,
    #[serde(default)]
    pub has_image_output: bool,
    #[serde(default)]
    pub has_executor: bool,
    #[serde(default)]
    pub image_output_active: bool,
    #[serde(default)]
    pub execution_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamRecordingDemandRuntimeState {
    #[serde(default)]
    pub recording_session_active: bool,
    #[serde(default)]
    pub shadow_recorder_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamDemandPipelineRuntimeState {
    #[serde(default)]
    pub decoded_image_active: bool,
    #[serde(default)]
    pub encoded_output_active: bool,
    #[serde(default)]
    pub preview_transport_active: bool,
    #[serde(default)]
    pub graph_image_output_active: bool,
    #[serde(default)]
    pub graph_execution_active: bool,
    #[serde(default)]
    pub encoded_passthrough_possible: bool,
    #[serde(default)]
    pub encoded_passthrough_active: bool,
    #[serde(default)]
    pub live_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamDemandRuntimeState {
    #[serde(default)]
    pub frame: StreamFrameDemandMetrics,
    #[serde(default)]
    pub encoder: StreamEncoderDemandMetrics,
    #[serde(default)]
    pub viewers: StreamViewerDemandRuntimeState,
    #[serde(default)]
    pub graph: StreamGraphDemandRuntimeState,
    #[serde(default)]
    pub recording: StreamRecordingDemandRuntimeState,
    #[serde(default)]
    pub pipeline: StreamDemandPipelineRuntimeState,
    #[serde(default)]
    pub live_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum StreamRecordingState {
    #[default]
    Inactive,
    Active,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamRecordingRuntimeState {
    #[serde(default)]
    pub state: StreamRecordingState,
    #[serde(default)]
    pub started_at_ms: Option<u64>,
}

impl Default for StreamRecordingRuntimeState {
    fn default() -> Self {
        Self { state: StreamRecordingState::Inactive, started_at_ms: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct StreamPipelineRuntimeState {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub active_pipeline_id: Option<Uuid>,
    #[serde(default)]
    pub active_output_key: Option<String>,
    #[serde(default)]
    pub pipeline_count: u64,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub disabled_since_ms: Option<u64>,
    #[serde(default)]
    pub disabled_reason: Option<String>,
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
pub const DEFAULT_STREAM_PIPELINE_ENABLED_WHEN_BINDINGS_PRESENT: bool = true;

fn default_preview_jpeg_quality_override() -> Option<u8> {
    env::var("HELIOS_PREVIEW_JPEG_QUALITY").ok().and_then(|v| v.parse::<u8>().ok())
}

pub fn default_requested_preview_jpeg_quality_enabled() -> u8 {
    default_preview_jpeg_quality_override().unwrap_or(DEFAULT_PREVIEW_JPEG_QUALITY).clamp(1, 100)
}

pub fn default_requested_preview_jpeg_quality_disabled() -> u8 {
    DEFAULT_STREAM_PREVIEW_JPEG_QUALITY
}

pub fn default_shadow_recording_codec() -> RecordingCodec {
    RecordingCodec::H264
}

pub fn default_recording_mode() -> StreamRecordingMode {
    StreamRecordingMode::Disabled
}

pub fn default_shadow_recorder_enabled() -> bool {
    default_recording_mode().is_shadow_buffer()
}

pub fn default_start_on_boot() -> bool {
    false
}

pub fn default_encoder_enabled() -> bool {
    true
}

pub fn default_decoder_enabled() -> bool {
    true
}

pub fn preview_format_for_encoder_selector(selector: Option<&str>) -> &'static str {
    match encoder_settings_kind_for_selector(selector) {
        Some(EncoderSettingsKind::Turbojpeg | EncoderSettingsKind::Mozjpeg | EncoderSettingsKind::FfmpegMjpeg) => "mjpeg",
        Some(EncoderSettingsKind::H264) => "h264",
        Some(EncoderSettingsKind::H265) => "h265",
        None => "unknown",
    }
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
    selector.eq_ignore_ascii_case("ffmpeg")
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
    if let Some(selector) = canonical_encoder_selector(manifest.encoder.id(), manifest.encoder.settings()) {
        manifest.encoder.set_id(Some(selector));
    }

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

pub fn stream_runtime_capabilities() -> Result<StreamRuntimeCapabilities, String> {
    let mut codecs: Vec<StreamCodecCapability> = CodecRegistry::list_enabled_codecs()
        .map_err(|err| err.to_string())?
        .into_iter()
        .flat_map(|(fourcc, descs)| {
            descs.into_iter().map(move |desc| {
                let tunables = if desc.kind == CodecKind::Encoder {
                    default_encoder_settings_for_codec(fourcc, desc.impl_name).map(|encoder_settings| StreamCodecTunables { encoder_settings: Some(encoder_settings) })
                } else {
                    None
                };
                StreamCodecCapability {
                    kind: desc.kind,
                    fourcc: fourcc.to_string(),
                    name: desc.name.to_string(),
                    implementation: desc.impl_name.to_string(),
                    input: desc.input.to_string(),
                    output: desc.output.to_string(),
                    tunables,
                }
            })
        })
        .collect();

    codecs.sort_by(|left, right| {
        let left_kind = match left.kind {
            CodecKind::Decoder => 0u8,
            CodecKind::Encoder => 1u8,
        };
        let right_kind = match right.kind {
            CodecKind::Decoder => 0u8,
            CodecKind::Encoder => 1u8,
        };
        left_kind
            .cmp(&right_kind)
            .then_with(|| left.fourcc.cmp(&right.fourcc))
            .then_with(|| left.input.cmp(&right.input))
            .then_with(|| left.output.cmp(&right.output))
            .then_with(|| left.implementation.cmp(&right.implementation))
    });

    Ok(StreamRuntimeCapabilities { codecs, default_encoder_id: default_stream_encoder_selector(), default_decoder_ids_by_capture_format: default_decoder_ids_by_capture_format() })
}

static STREAM_RUNTIME_CAPABILITIES_CACHE: OnceLock<Result<StreamRuntimeCapabilities, String>> = OnceLock::new();

pub fn cached_stream_runtime_capabilities() -> Result<StreamRuntimeCapabilities, String> {
    STREAM_RUNTIME_CAPABILITIES_CACHE.get_or_init(stream_runtime_capabilities).clone()
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
    let Some(kind) = encoder_settings_kind_for_selector(encoder.codec_id.as_deref()) else {
        return;
    };
    if !matches!(kind, EncoderSettingsKind::FfmpegMjpeg | EncoderSettingsKind::H264 | EncoderSettingsKind::H265) {
        return;
    }
    let settings = encoder.settings.get_or_insert_with(|| match kind {
        EncoderSettingsKind::FfmpegMjpeg => EncoderSettings::FfmpegMjpeg { bitrate: None, gop: None, framerate: None, thread_count: None, output_resolution: None },
        EncoderSettingsKind::H264 => EncoderSettings::H264 { bitrate: None, gop: None, framerate: None, thread_count: None, output_resolution: None },
        EncoderSettingsKind::H265 => EncoderSettings::H265 { bitrate: None, gop: None, framerate: None, thread_count: None, output_resolution: None },
        EncoderSettingsKind::Turbojpeg | EncoderSettingsKind::Mozjpeg => unreachable!("video defaults are only applied to video encoder settings"),
    });
    settings.ensure_video_framerate(FrameRate { numerator: DEFAULT_STREAM_ENCODER_FPS, denominator: 1 });
    settings.ensure_video_output_resolution(default_encoder_output_resolution(capture.mode.format.resolution));
}
