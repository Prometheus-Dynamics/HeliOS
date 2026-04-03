use super::*;
use crate::ipc::CalibrationSolveResponse;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingContainer {
    Mp4,
    Raw,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingCodec {
    H264,
    H265,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordingSource {
    #[default]
    Multiplex,
    Raw,
    Pipeline {
        #[serde(default)]
        pipeline_id: Option<Uuid>,
        #[serde(default)]
        output_key: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct StreamCodecTunables {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoder_settings: Option<EncoderSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct StreamCodecCapability {
    #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
    pub kind: CodecKind,
    pub fourcc: String,
    pub name: String,
    pub implementation: String,
    pub input: String,
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunables: Option<StreamCodecTunables>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamRuntimeCapabilities {
    #[serde(default)]
    pub codecs: Vec<StreamCodecCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_encoder_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub default_decoder_ids_by_capture_format: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct FrameRate {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ResolutionHint {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum EncoderSettingsKind {
    Turbojpeg,
    Mozjpeg,
    FfmpegMjpeg,
    H264,
    H265,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
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

    pub(super) fn ensure_video_framerate(&mut self, default_framerate: FrameRate) {
        match self {
            Self::Turbojpeg { .. } | Self::Mozjpeg { .. } => {}
            Self::FfmpegMjpeg { framerate, .. } | Self::H264 { framerate, .. } | Self::H265 { framerate, .. } => {
                if framerate.is_none() {
                    *framerate = Some(default_framerate);
                }
            }
        }
    }

    pub(super) fn ensure_video_output_resolution(&mut self, default_resolution: ResolutionHint) {
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

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub(super) enum EncoderSettingsBinaryWire {
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

pub(super) fn deserialize_encoder_frame_rate<'de, D>(deserializer: D) -> Result<Option<FrameRate>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<FrameRate>::deserialize(deserializer)
}

pub(super) fn encoder_settings_kind_for_family_id(family_id: &str) -> Option<EncoderSettingsKind> {
    match family_id {
        "turbojpeg" => Some(EncoderSettingsKind::Turbojpeg),
        "mozjpeg" => Some(EncoderSettingsKind::Mozjpeg),
        "ffmpeg_mjpeg" => Some(EncoderSettingsKind::FfmpegMjpeg),
        "h264" => Some(EncoderSettingsKind::H264),
        "h265" => Some(EncoderSettingsKind::H265),
        _ => None,
    }
}

pub(super) fn encoder_settings_kind_for_selector(selector: Option<&str>) -> Option<EncoderSettingsKind> {
    let selector = selector.map(str::trim).filter(|value| !value.is_empty())?;
    styx_encoder_family_for_selector(selector).and_then(|spec| encoder_settings_kind_for_family_id(spec.id))
}

pub(super) fn selector_name_for_encoder_settings_kind(kind: EncoderSettingsKind) -> &'static str {
    match kind {
        EncoderSettingsKind::Turbojpeg => "turbojpeg",
        EncoderSettingsKind::Mozjpeg => "mozjpeg",
        EncoderSettingsKind::FfmpegMjpeg => "mjpeg",
        EncoderSettingsKind::H264 => "h264",
        EncoderSettingsKind::H265 => "h265",
    }
}

pub(super) fn default_encoder_settings_for_kind(kind: EncoderSettingsKind) -> EncoderSettings {
    match kind {
        EncoderSettingsKind::Turbojpeg => EncoderSettings::Turbojpeg { quality: Some(85) },
        EncoderSettingsKind::Mozjpeg => EncoderSettings::Mozjpeg { quality: Some(85) },
        EncoderSettingsKind::FfmpegMjpeg => EncoderSettings::FfmpegMjpeg {
            bitrate: Some(4_000_000),
            gop: None,
            framerate: Some(FrameRate { numerator: 60, denominator: 1 }),
            thread_count: None,
            output_resolution: Some(ResolutionHint { width: 854, height: 480 }),
        },
        EncoderSettingsKind::H264 => EncoderSettings::H264 {
            bitrate: Some(4_000_000),
            gop: None,
            framerate: Some(FrameRate { numerator: 60, denominator: 1 }),
            thread_count: None,
            output_resolution: Some(ResolutionHint { width: 854, height: 480 }),
        },
        EncoderSettingsKind::H265 => EncoderSettings::H265 {
            bitrate: Some(4_000_000),
            gop: None,
            framerate: Some(FrameRate { numerator: 60, denominator: 1 }),
            thread_count: None,
            output_resolution: Some(ResolutionHint { width: 854, height: 480 }),
        },
    }
}

pub fn default_encoder_settings_for_codec(fourcc: FourCc, implementation: &str) -> Option<EncoderSettings> {
    let runtime = styx_runtime_codec_inventory().ok()?;
    let family_id = runtime
        .codecs
        .into_iter()
        .find(|codec| codec.kind == CodecKind::Encoder && codec.fourcc.eq_ignore_ascii_case(&fourcc.to_string()) && codec.implementation.eq_ignore_ascii_case(implementation))
        .and_then(|codec| codec.family_id)?;
    encoder_settings_kind_for_family_id(family_id).map(default_encoder_settings_for_kind)
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

pub(super) fn canonical_encoder_selector(selector: Option<&str>, settings: Option<&EncoderSettings>) -> Option<String> {
    let selector = selector.map(str::trim).filter(|value| !value.is_empty())?;
    if selector.eq_ignore_ascii_case("ffmpeg") {
        return settings.map(EncoderSettings::settings_kind).map(selector_name_for_encoder_settings_kind).map(ToString::to_string);
    }

    match selector.to_ascii_lowercase().as_str() {
        "avc" => Some("h264".to_string()),
        "hevc" => Some("h265".to_string()),
        "mjpg" | "jpeg" | "ffmpeg_mjpeg" => Some("mjpeg".to_string()),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema, Archive, RkyvSerialize, RkyvDeserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum EngineEvent {
    Ack {
        command_id: CommandId,
        ok: bool,
    },
    Nack {
        command_id: CommandId,
        code: EngineErrorCode,
        reason: String,
        retryable: bool,
    },
    StreamList {
        command_id: CommandId,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        streams: Vec<StreamSummary>,
    },
    StreamRuntimeCapabilities {
        command_id: CommandId,
        capabilities: StreamRuntimeCapabilities,
    },
    Started {
        command_id: CommandId,
        stream_id: Uuid,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        descriptor: CaptureDescriptor,
    },
    Stopped {
        command_id: CommandId,
        stream_id: Uuid,
    },
    Controls {
        command_id: CommandId,
        stream_id: Uuid,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        controls: Vec<CaptureControlInfo>,
    },
    Metrics {
        command_id: CommandId,
        stream_id: Uuid,
        metrics: StreamMetrics,
    },
    SnapshotJpeg {
        command_id: CommandId,
        stream_id: Uuid,
        quality: u8,
        bytes: Vec<u8>,
    },
    GraphOutputs {
        command_id: CommandId,
        stream_id: Uuid,
        outputs: Vec<GraphOutputPortDescriptor>,
    },
    GraphOutputSample {
        command_id: CommandId,
        stream_id: Uuid,
        port: String,
        value: JsonWire,
    },
    /// Unsolicited metrics update broadcast by the engine.
    MetricsUpdate {
        stream_id: Uuid,
        metrics: StreamMetrics,
    },
    NodeRegistry {
        command_id: CommandId,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        snapshot: NodeRegistrySnapshot,
    },
    Discovery {
        command_id: CommandId,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        discovery: crate::capture::DiscoveryResult,
    },
    GraphValidation {
        command_id: CommandId,
        report: GraphValidationReport,
    },
    CalibrationSolved {
        command_id: CommandId,
        #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
        response: CalibrationSolveResponse,
    },
    LocalizationSolved {
        command_id: CommandId,
        response: JsonWire,
    },
    LocalizationPipelineStatus {
        command_id: CommandId,
        response: JsonWire,
    },
    LocalizationPipelineOutputs {
        command_id: CommandId,
        outputs: Vec<String>,
    },
    LocalizationPipelineOutputSample {
        command_id: CommandId,
        response: JsonWire,
    },
}
