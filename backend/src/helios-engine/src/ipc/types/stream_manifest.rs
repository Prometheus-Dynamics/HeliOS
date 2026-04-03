use super::*;

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
