use super::*;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq, Default)]
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

pub(super) mod stream_recording_mode_serde {
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
pub(super) enum RequestedEncoderConfigBinaryWire {
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
pub(super) enum RequestedEncoderConfigHumanWire {
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
    pub(super) fn into_requested(self) -> Result<(RequestedEncoderConfig, Option<f64>), String> {
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
pub(super) enum RequestedDecoderConfigBinaryWire {
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
pub(super) enum RequestedDecoderConfigHumanWire {
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

pub(super) fn canonical_requested_host_buffer(value: usize) -> usize {
    if value == 0 {
        default_host_buffer()
    } else {
        value
    }
}

pub(super) fn canonical_requested_pipeline_enabled(
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

pub(super) fn canonical_requested_preview_jpeg_quality(value: Option<u8>, encoder: &RequestedEncoderConfig) -> u8 {
    value.map(|value| value.clamp(1, 100)).unwrap_or_else(|| default_requested_preview_jpeg_quality(encoder))
}
