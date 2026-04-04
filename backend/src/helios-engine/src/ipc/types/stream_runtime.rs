use super::*;
use lib_runtime_policy::HELIOS_ENGINE_STREAM_RUNTIME_POLICY;

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
    // still override via runtime policy or per-stream `host_buffer`.
    HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve().host_buffer_default
}

const DEFAULT_PREVIEW_JPEG_QUALITY: u8 = 65;
const DEFAULT_STREAM_ENCODER_FPS: u32 = 60;
const DEFAULT_STREAM_ENCODER_OUTPUT_HEIGHT: u32 = 480;
const DEFAULT_STREAM_PREVIEW_JPEG_QUALITY: u8 = 30;
pub const DEFAULT_STREAM_PIPELINE_ENABLED_WHEN_BINDINGS_PRESENT: bool = true;

fn default_preview_jpeg_quality_override() -> Option<u8> {
    HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve().preview_jpeg_quality_override
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
    styx_preview_format_for_encoder_selector(selector)
}

pub(super) fn max_host_buffer() -> usize {
    HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve().host_buffer_max
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
    styx_default_stream_encoder_selector()
}

pub fn normalize_requested_stream_encoder(manifest: &mut StreamManifest) {
    normalize_stream_encoder_selection(manifest);
}

pub fn normalize_requested_stream_decoder(manifest: &mut StreamManifest) {
    normalize_stream_decoder_selection(manifest);
}

pub(super) fn normalize_stream_encoder_selection(manifest: &mut StreamManifest) {
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

pub(super) fn normalize_stream_decoder_selection(manifest: &mut StreamManifest) {
    if manifest.decoder.is_disabled() {
        return;
    }

    let Some(default_selector) = default_decoder_selector_for_capture_format(manifest.capture.mode.format.code) else {
        return;
    };
    manifest.decoder.ensure_id(Some(default_selector));
}

pub(super) fn normalized_codec_selector(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(ToString::to_string)
}

pub fn default_decoder_ids_by_capture_format() -> BTreeMap<String, String> {
    styx_default_decoder_ids_by_capture_format()
}

pub fn default_decoder_selector_for_capture_format(fourcc: FourCc) -> Option<String> {
    styx_default_decoder_selector_for_capture_format(fourcc)
}

pub fn stream_runtime_capabilities() -> Result<StreamRuntimeCapabilities, String> {
    let runtime = styx_runtime_codec_inventory().map_err(|err| err.to_string())?;
    let codecs = runtime
        .codecs
        .into_iter()
        .map(|codec| {
            let tunables = if codec.kind == CodecKind::Encoder {
                default_encoder_settings_for_codec(FourCc::from_str(&codec.output).unwrap_or_else(|_| FourCc::new(*b"ANY ")), &codec.implementation)
                    .map(|encoder_settings| StreamCodecTunables { encoder_settings: Some(encoder_settings) })
            } else {
                None
            };
            StreamCodecCapability { kind: codec.kind, fourcc: codec.fourcc, name: codec.name, implementation: codec.implementation, input: codec.input, output: codec.output, tunables }
        })
        .collect();

    Ok(StreamRuntimeCapabilities { codecs, default_encoder_id: runtime.default_encoder_selector, default_decoder_ids_by_capture_format: runtime.default_decoder_ids_by_capture_format })
}

#[derive(Debug, Clone, Serialize, ToSchema, Default)]
pub struct StreamRuntimeCapabilitiesCacheSnapshot {
    pub entries: u64,
    pub hits: u64,
    pub misses: u64,
    pub refreshes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Default)]
struct StreamRuntimeCapabilitiesCacheState {
    cached: Option<Result<StreamRuntimeCapabilities, String>>,
    hits: u64,
    misses: u64,
    refreshes: u64,
}

fn stream_runtime_capabilities_cache() -> &'static Mutex<StreamRuntimeCapabilitiesCacheState> {
    static CACHE: LazyLock<Mutex<StreamRuntimeCapabilitiesCacheState>> = LazyLock::new(|| Mutex::new(StreamRuntimeCapabilitiesCacheState::default()));
    &CACHE
}

pub fn cached_stream_runtime_capabilities() -> Result<StreamRuntimeCapabilities, String> {
    {
        let mut state = stream_runtime_capabilities_cache().lock().expect("stream runtime capabilities cache poisoned");
        if let Some(cached) = state.cached.clone() {
            state.hits = state.hits.saturating_add(1);
            return cached;
        }
        state.misses = state.misses.saturating_add(1);
    }

    let computed = stream_runtime_capabilities();
    let mut state = stream_runtime_capabilities_cache().lock().expect("stream runtime capabilities cache poisoned");
    if let Some(cached) = state.cached.clone() {
        state.hits = state.hits.saturating_add(1);
        return cached;
    }
    state.refreshes = state.refreshes.saturating_add(1);
    state.cached = Some(computed.clone());
    computed
}

pub fn stream_runtime_capabilities_cache_snapshot() -> StreamRuntimeCapabilitiesCacheSnapshot {
    let state = stream_runtime_capabilities_cache().lock().expect("stream runtime capabilities cache poisoned");
    StreamRuntimeCapabilitiesCacheSnapshot {
        entries: u64::from(state.cached.is_some()),
        hits: state.hits,
        misses: state.misses,
        refreshes: state.refreshes,
        last_error: state.cached.as_ref().and_then(|cached| cached.as_ref().err().cloned()),
    }
}

#[cfg(test)]
pub(crate) fn reset_stream_runtime_capabilities_cache_for_tests() {
    *stream_runtime_capabilities_cache().lock().expect("stream runtime capabilities cache poisoned") = StreamRuntimeCapabilitiesCacheState::default();
}

pub(super) fn default_encoder_output_resolution(capture_resolution: Resolution) -> ResolutionHint {
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

pub(super) fn apply_default_encoder_settings(capture: &CaptureConfig, encoder: &mut ResolvedEncoderConfig) {
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
