use helios_engine::capture::CaptureDescriptor;
use helios_engine::ipc::{EncoderSettings, EngineErrorCode, ResolvedStreamConfig, StreamManifest, StreamRuntimeState, StreamStatus};
use serde::{Deserialize, Serialize};
use styx::codec::CodecKind;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone)]
pub struct StreamInfo {
    pub id: Uuid,
    pub descriptor: CaptureDescriptor,
    pub manifest: StreamManifest,
    pub resolved: ResolvedStreamConfig,
    #[serde(default)]
    pub status: Option<StreamStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<StreamRuntimeState>,
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
    pub encoder_settings: Option<EncoderSettings>,
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
