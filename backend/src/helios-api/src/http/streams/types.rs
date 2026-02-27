use helios_engine::capture::CaptureDescriptor;
use helios_engine::ipc::{EngineErrorCode, FrameRate, ResolutionHint, StreamManifest, StreamStatus};
use serde::{Deserialize, Serialize};
use styx::codec::CodecKind;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, Deserialize, ToSchema, Clone)]
pub struct StreamInfo {
    pub id: Uuid,
    pub descriptor: CaptureDescriptor,
    pub manifest: StreamManifest,
    #[serde(default)]
    pub status: Option<StreamStatus>,
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
    pub encoder_settings: Option<EncoderSettingsDescriptor>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Default)]
pub struct EncoderSettingsDescriptor {
    #[serde(default)]
    pub bitrate: Option<u64>,
    #[serde(default)]
    pub gop: Option<i32>,
    #[serde(default)]
    pub framerate: Option<FrameRate>,
    #[serde(default)]
    pub thread_count: Option<usize>,
    #[serde(default)]
    pub output_resolution: Option<ResolutionHint>,
    #[serde(default)]
    pub decode_fps_limit: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EngineErrorBody {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_code: Option<EngineErrorCode>,
    pub error: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StreamFormatInfo {
    pub fourcc: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
}
