use helios_engine::capture::CaptureDescriptor;
use helios_engine::ipc::{
    EngineErrorCode, ResolvedStreamConfig, StreamCaptureRuntimeState, StreamCodecCapability, StreamCodecChainRuntimeState, StreamCodecTunables as RuntimeCodecTunables, StreamDemandRuntimeState,
    StreamManifest, StreamPipelineRuntimeState, StreamRecordingRuntimeState, StreamRuntimeState, StreamStatus,
};
use serde::{Deserialize, Serialize};
use styx::codec::CodecKind;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamPreviewFormat {
    Mjpeg,
    H264,
    H265,
    Unknown,
}

impl StreamPreviewFormat {
    pub fn from_encoder_selector(selector: Option<&str>) -> Self {
        match helios_engine::ipc::preview_format_for_encoder_selector(selector) {
            "mjpeg" => Self::Mjpeg,
            "h264" => Self::H264,
            "h265" => Self::H265,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct StreamInfo {
    pub id: Uuid,
    pub descriptor: CaptureDescriptor,
    pub manifest: StreamManifest,
    pub resolved: ResolvedStreamConfig,
    pub preview_format: StreamPreviewFormat,
    #[serde(default)]
    pub status: Option<StreamStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<StreamRuntimeState>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct StreamInspectInfo {
    pub id: Uuid,
    pub descriptor: CaptureDescriptor,
    pub resolved: ResolvedStreamConfig,
    pub preview_format: StreamPreviewFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<StreamCaptureRuntimeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_chain: Option<StreamCodecChainRuntimeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_demand: Option<StreamDemandRuntimeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording: Option<StreamRecordingRuntimeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<StreamPipelineRuntimeState>,
}

impl From<StreamInfo> for StreamInspectInfo {
    fn from(value: StreamInfo) -> Self {
        let capture = value.runtime.as_ref().map(|runtime| runtime.capture.clone());
        let codec_chain = value.runtime.as_ref().map(|runtime| runtime.codecs.clone());
        let consumer_demand = value.runtime.as_ref().map(|runtime| runtime.demand.clone());
        let recording = value.runtime.as_ref().map(|runtime| runtime.recording.clone());
        let pipeline = value.runtime.as_ref().map(|runtime| runtime.pipeline.clone());
        Self { id: value.id, descriptor: value.descriptor, resolved: value.resolved, preview_format: value.preview_format, capture, codec_chain, consumer_demand, recording, pipeline }
    }
}

#[derive(Serialize, Deserialize, ToSchema)]
pub struct StartStreamResponse {
    pub stream_id: Uuid,
    pub descriptor: CaptureDescriptor,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CodecInfo {
    pub kind: CodecKind,
    pub fourcc: String,
    pub name: String,
    pub implementation: String,
    pub input: String,
    pub output: String,
    #[serde(default)]
    pub tunables: Option<CodecTunables>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct CodecTunables {
    #[serde(default)]
    pub encoder_settings: Option<helios_engine::ipc::EncoderSettings>,
}

impl From<RuntimeCodecTunables> for CodecTunables {
    fn from(value: RuntimeCodecTunables) -> Self {
        Self { encoder_settings: value.encoder_settings }
    }
}

impl From<StreamCodecCapability> for CodecInfo {
    fn from(value: StreamCodecCapability) -> Self {
        Self {
            kind: value.kind,
            fourcc: value.fourcc,
            name: value.name,
            implementation: value.implementation,
            input: value.input,
            output: value.output,
            tunables: value.tunables.map(CodecTunables::from),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EngineErrorBody {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_code: Option<EngineErrorCode>,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StreamFormatInfo {
    pub fourcc: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
}
