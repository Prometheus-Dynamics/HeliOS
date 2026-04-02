#[cfg(test)]
use helios_engine::ipc::default_shadow_recording_codec;
use helios_engine::ipc::{
    DEFAULT_STREAM_PIPELINE_ENABLED_WHEN_BINDINGS_PRESENT, EncoderSettings, ResolvedStreamConfig, StreamManifest, StreamRecordingMode, StreamRuntimeCapabilities, default_decoder_enabled,
    default_encoder_enabled, default_host_buffer, default_recording_mode, default_requested_preview_jpeg_quality_disabled, default_requested_preview_jpeg_quality_enabled, default_start_on_boot,
    empty_encoder_settings_for_selector, normalize_pipeline_output_selection, stream_runtime_capabilities,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use styx::codec::CodecKind;
use styx::{BackendHandle, BackendKind};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::pipelines;
use crate::http::validation::{ValidationIssue, ValidationWarning, issue, issue_with_remediation, warning};

use super::util;
use super::{CALIBRATION_MODE_PIPELINE_UUID, RAW_PIPELINE_UUID};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamValidateResponse {
    pub manifest: StreamManifest,
    pub resolved: ResolvedStreamConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamCapabilitiesResponse {
    pub raw_pipeline_id: Uuid,
    pub calibration_mode_pipeline_id: Uuid,
    pub defaults: StreamValidationDefaults,
    pub constraints: StreamValidationConstraints,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamValidationDefaults {
    pub raw_output: String,
    pub undistorted_output: String,
    pub pipeline_enabled_when_bindings_present: bool,
    pub default_encoder_enabled: bool,
    pub default_decoder_enabled: bool,
    pub default_host_buffer: usize,
    pub default_preview_jpeg_quality: u8,
    pub default_preview_jpeg_quality_when_encoder_disabled: u8,
    pub default_recording_mode: StreamRecordingMode,
    pub default_start_on_boot: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_encoder_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub default_decoder_ids_by_capture_format: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamValidationConstraints {
    pub requires_backend_handle_match: bool,
    pub file_backend_requires_non_empty_paths: bool,
    pub file_backend_supported_extensions: Vec<String>,
    pub min_layout_rows: u8,
    pub min_layout_columns: u8,
    pub reserved_pipeline_ids: Vec<Uuid>,
    pub raw_output_values: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct StreamValidationResult {
    pub manifest: StreamManifest,
    pub resolved: ResolvedStreamConfig,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone)]
pub struct NormalizedStreamManifest {
    pub manifest: StreamManifest,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone)]
pub struct StreamValidationError {
    pub issues: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum EncoderSettingsShape {
    Turbojpeg,
    Mozjpeg,
    FfmpegMjpeg,
    H264,
    H265,
}

const FILE_BACKEND_SUPPORTED_EXTENSIONS: &[&str] =
    &["bmp", "gif", "heic", "heif", "jpeg", "jpg", "png", "tif", "tiff", "webp", "h264", "avc", "h265", "hevc", "m4v", "mjpeg", "mjpg", "mov", "mp4", "mpe", "mpeg", "mpg", "webm", "wmv", "y4m"];

pub fn stream_capabilities(runtime: &StreamRuntimeCapabilities) -> StreamCapabilitiesResponse {
    StreamCapabilitiesResponse {
        raw_pipeline_id: RAW_PIPELINE_UUID,
        calibration_mode_pipeline_id: CALIBRATION_MODE_PIPELINE_UUID,
        defaults: StreamValidationDefaults {
            raw_output: "raw".to_string(),
            undistorted_output: "undistorted".to_string(),
            pipeline_enabled_when_bindings_present: DEFAULT_STREAM_PIPELINE_ENABLED_WHEN_BINDINGS_PRESENT,
            default_encoder_enabled: default_encoder_enabled(),
            default_decoder_enabled: default_decoder_enabled(),
            default_host_buffer: default_host_buffer(),
            default_preview_jpeg_quality: default_requested_preview_jpeg_quality_enabled(),
            default_preview_jpeg_quality_when_encoder_disabled: default_requested_preview_jpeg_quality_disabled(),
            default_recording_mode: default_recording_mode(),
            default_start_on_boot: default_start_on_boot(),
            default_encoder_id: runtime.default_encoder_id.clone(),
            default_decoder_ids_by_capture_format: runtime.default_decoder_ids_by_capture_format.clone(),
        },
        constraints: StreamValidationConstraints {
            requires_backend_handle_match: true,
            file_backend_requires_non_empty_paths: true,
            file_backend_supported_extensions: FILE_BACKEND_SUPPORTED_EXTENSIONS.iter().map(|value| value.to_string()).collect(),
            min_layout_rows: 1,
            min_layout_columns: 1,
            reserved_pipeline_ids: vec![CALIBRATION_MODE_PIPELINE_UUID],
            raw_output_values: vec!["raw".to_string(), "undistorted".to_string()],
        },
    }
}

pub fn normalize_stream_manifest(mut manifest: StreamManifest) -> NormalizedStreamManifest {
    let mut warnings = Vec::<ValidationWarning>::new();

    normalize_file_stream_identity(&mut manifest);
    normalize_file_backend_paths(&mut manifest, &mut warnings);
    normalize_file_capture_manifest(&mut manifest);
    normalize_reserved_pipeline_ids(&mut manifest, &mut warnings);
    normalize_pipeline_output_fields(&mut manifest, &mut warnings);
    util::normalize_stream_encoder_manifest(&mut manifest);
    normalize_capture_tdn_output(&mut manifest);

    NormalizedStreamManifest { manifest, warnings }
}

pub async fn validate_stream_manifest(manifest: StreamManifest) -> Result<StreamValidationResult, StreamValidationError> {
    let runtime = stream_runtime_capabilities().map_err(|err| StreamValidationError {
        issues: vec![issue("/", "runtime_capabilities_unavailable", format!("stream runtime capability inventory unavailable: {err}"))],
        warnings: Vec::new(),
    })?;
    validate_stream_manifest_with_runtime(manifest, &runtime).await
}

pub async fn validate_stream_manifest_with_runtime(manifest: StreamManifest, runtime: &StreamRuntimeCapabilities) -> Result<StreamValidationResult, StreamValidationError> {
    let NormalizedStreamManifest { manifest, warnings } = normalize_stream_manifest(manifest);
    let mut issues = Vec::<ValidationIssue>::new();

    validate_backend_and_handle(&manifest, &mut issues);
    validate_explicit_stream_config(&manifest, &mut issues);
    validate_file_backend_paths_present(&manifest, &mut issues);
    validate_file_backend_media_paths(&manifest, &mut issues).await;
    validate_pipeline_layout(&manifest, &mut issues);
    validate_pipeline_wires(&manifest, &mut issues);
    validate_raw_pipeline_output_values(&manifest, &mut issues);
    validate_pipeline_bindings(&manifest, &mut issues).await;
    validate_stream_feature_compatibility(&manifest, runtime, &mut issues);

    if issues.is_empty() {
        let mut manifest = manifest;
        util::normalize_pipeline_manifest(&mut manifest);
        let resolved = manifest.resolve();
        Ok(StreamValidationResult { manifest, resolved, warnings })
    } else {
        Err(StreamValidationError { issues, warnings })
    }
}

fn validate_explicit_stream_config(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    if manifest.host_buffer == 0 {
        issues.push(issue("/host_buffer", "host_buffer_must_be_positive", "host_buffer must be greater than zero"));
    }

    if manifest.preview_jpeg_quality == 0 {
        issues.push(issue("/preview_jpeg_quality", "preview_quality_must_be_positive", "preview_jpeg_quality must be between 1 and 100"));
    }

    let has_disabled_pipeline_state = !manifest.pipelines.is_empty()
        || manifest.active_pipeline_id.is_some()
        || manifest.active_pipeline_output.as_deref().is_some_and(|value| !value.trim().is_empty())
        || manifest.pipeline_layout.is_some()
        || !manifest.pipeline_wires.is_empty();
    if !manifest.pipeline_enabled && has_disabled_pipeline_state {
        issues.push(issue("/pipeline_enabled", "pipeline_disabled_with_pipeline_state", "pipeline_enabled=false requires pipelines, active pipeline selection, layout, and wires to be empty"));
    }
}

fn validate_backend_and_handle(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    let compatible = matches!(
        (manifest.capture.backend, &manifest.capture.handle),
        (BackendKind::V4l2, BackendHandle::V4l2 { .. })
            | (BackendKind::Libcamera, BackendHandle::Libcamera { .. })
            | (BackendKind::Virtual, BackendHandle::Virtual)
            | (BackendKind::Netcam, BackendHandle::Netcam { .. })
            | (BackendKind::File, BackendHandle::File { .. })
    );

    if !compatible {
        issues.push(issue(
            "/capture/handle",
            "backend_handle_mismatch",
            format!("capture backend `{}` does not match handle variant `{}`", backend_label(manifest.capture.backend), handle_label(&manifest.capture.handle)),
        ));
    }
}

fn validate_stream_feature_compatibility(manifest: &StreamManifest, runtime: &StreamRuntimeCapabilities, issues: &mut Vec<ValidationIssue>) {
    validate_requested_codec_compatibility(manifest, runtime, issues);
    validate_recording_mode_compatibility(manifest, runtime, issues);
}

fn validate_requested_codec_compatibility(manifest: &StreamManifest, runtime: &StreamRuntimeCapabilities, issues: &mut Vec<ValidationIssue>) {
    if !manifest.encoder.is_disabled() {
        validate_codec_selector_available(styx::prelude::FourCc::new(*b"RG24"), CodecKind::Encoder, manifest.encoder.id(), runtime, "/encoder/id", "encoder_unavailable", issues);
        validate_encoder_settings_compatibility(manifest, runtime, issues);
    }

    if !manifest.decoder.is_disabled() {
        let capture_fourcc = manifest.capture.mode.format.code;
        validate_codec_selector_available(capture_fourcc, CodecKind::Decoder, manifest.decoder.id(), runtime, "/decoder/id", "decoder_unavailable", issues);
    }
}

fn validate_encoder_settings_compatibility(manifest: &StreamManifest, _runtime: &StreamRuntimeCapabilities, issues: &mut Vec<ValidationIssue>) {
    let Some(settings) = manifest.encoder.settings() else {
        return;
    };
    let Some(selector) = manifest.encoder.id().map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    let Some(expected) = expected_encoder_settings_shape(selector) else {
        return;
    };

    if encoder_settings_shape(settings) != expected {
        issues.push(issue_with_remediation(
            "/encoder/settings/kind",
            "encoder_settings_kind_mismatch",
            format!("encoder settings kind does not match encoder `{selector}`"),
            "Choose settings that match the selected encoder family, or switch the encoder selector to match the requested settings kind.",
        ));
        return;
    }

    validate_encoder_settings_values(settings, issues);
}

fn validate_encoder_settings_values(settings: &EncoderSettings, issues: &mut Vec<ValidationIssue>) {
    match settings {
        EncoderSettings::Turbojpeg { quality } | EncoderSettings::Mozjpeg { quality } => {
            if let Some(quality) = quality
                && !(*quality >= 1 && *quality <= 100)
            {
                issues.push(issue("/encoder/settings/quality", "encoder_quality_out_of_range", "encoder quality must be between 1 and 100"));
            }
        }
        EncoderSettings::FfmpegMjpeg { bitrate, gop, framerate, thread_count, output_resolution }
        | EncoderSettings::H264 { bitrate, gop, framerate, thread_count, output_resolution }
        | EncoderSettings::H265 { bitrate, gop, framerate, thread_count, output_resolution } => {
            if let Some(bitrate) = bitrate
                && *bitrate == 0
            {
                issues.push(issue("/encoder/settings/bitrate", "encoder_bitrate_must_be_positive", "encoder bitrate must be greater than zero"));
            }
            if let Some(gop) = gop
                && *gop <= 0
            {
                issues.push(issue("/encoder/settings/gop", "encoder_gop_must_be_positive", "encoder gop must be greater than zero"));
            }
            if let Some(framerate) = framerate
                && (framerate.numerator == 0 || framerate.denominator == 0)
            {
                issues.push(issue("/encoder/settings/framerate", "encoder_framerate_invalid", "encoder framerate numerator and denominator must be greater than zero"));
            }
            if let Some(thread_count) = thread_count
                && *thread_count == 0
            {
                issues.push(issue("/encoder/settings/thread_count", "encoder_thread_count_must_be_positive", "encoder thread_count must be greater than zero"));
            }
            if let Some(output_resolution) = output_resolution
                && (output_resolution.width == 0 || output_resolution.height == 0)
            {
                issues.push(issue("/encoder/settings/output_resolution", "encoder_output_resolution_invalid", "encoder output resolution width and height must be greater than zero"));
            }
        }
    }
}

fn validate_codec_selector_available(
    input: styx::prelude::FourCc,
    kind: CodecKind,
    selector: Option<&str>,
    runtime: &StreamRuntimeCapabilities,
    pointer: &str,
    code: &'static str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(selector) = selector.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };

    if codec_selector_available(runtime, input, kind, selector) {
        return;
    }

    let kind_label = match kind {
        CodecKind::Encoder => "encoder",
        CodecKind::Decoder => "decoder",
    };
    let format_label = String::from_utf8_lossy(&input.to_u32().to_le_bytes()).trim().to_string();
    let remediation = match kind {
        CodecKind::Encoder => "Choose an available encoder selector for this stream, or disable encoding.",
        CodecKind::Decoder => "Choose a decoder selector that supports the selected capture format, or disable decoding.",
    };
    issues.push(issue_with_remediation(pointer, code, format!("{kind_label} `{selector}` is not available for capture format {format_label}"), remediation));
}

fn codec_selector_available(runtime: &StreamRuntimeCapabilities, input: styx::prelude::FourCc, kind: CodecKind, selector: &str) -> bool {
    runtime.codecs.iter().any(|codec| codec.kind == kind && runtime_codec_matches_input(codec, input) && runtime_codec_matches_selector(codec, selector))
}

fn validate_recording_mode_compatibility(manifest: &StreamManifest, runtime: &StreamRuntimeCapabilities, issues: &mut Vec<ValidationIssue>) {
    let Some(requested_codec) = manifest.recording_mode.shadow_buffer_codec() else {
        return;
    };

    if !crate::features::shadow_recorder_enabled() {
        issues.push(issue_with_remediation(
            "/recording_mode/state",
            "recording_mode_feature_disabled",
            "shadow-buffer recording mode is disabled by the HELIOS_ENABLE_SHADOW_RECORDER feature gate",
            "Switch recording mode to disabled, or enable HELIOS_ENABLE_SHADOW_RECORDER before retrying.",
        ));
        return;
    }

    if manifest.encoder.is_disabled() {
        issues.push(issue_with_remediation(
            "/recording_mode/state",
            "recording_mode_requires_encoder",
            "shadow-buffer recording mode requires the stream encoder to be enabled",
            "Enable the stream encoder before turning on shadow-buffer recording.",
        ));
        return;
    }

    let Some(encoder_id) = manifest.encoder.id().map(str::trim).filter(|value| !value.is_empty()) else {
        issues.push(issue_with_remediation(
            "/encoder/id",
            "recording_mode_requires_matching_encoder",
            format!("recording mode requests {:?} but the stream encoder selector is missing", requested_codec).to_lowercase(),
            "Select an H264 or H265 encoder that matches the chosen recording mode codec.",
        ));
        return;
    };

    if !encoder_matches_recording_codec(runtime, requested_codec, encoder_id) {
        issues.push(issue_with_remediation(
            "/recording_mode/codec",
            "recording_mode_codec_mismatch",
            format!("recording mode codec {:?} does not match encoder `{encoder_id}`", requested_codec).to_lowercase(),
            "Choose an encoder selector that matches the requested recording mode codec, or switch the recording mode codec to match the encoder.",
        ));
    }
}

fn encoder_matches_recording_codec(runtime: &StreamRuntimeCapabilities, codec: helios_engine::ipc::RecordingCodec, encoder_id: &str) -> bool {
    let targets: &[&str] = match codec {
        helios_engine::ipc::RecordingCodec::H264 => &["h264", "avc"],
        helios_engine::ipc::RecordingCodec::H265 => &["h265", "hevc"],
    };
    let encoder_id = encoder_id.trim();
    if encoder_id.is_empty() {
        return false;
    }
    if targets.iter().any(|target| encoder_id.eq_ignore_ascii_case(target)) {
        return true;
    }
    let lowered = encoder_id.to_ascii_lowercase();
    if targets.iter().any(|target| lowered.contains(target)) {
        return true;
    }

    let mut matched_names = BTreeSet::new();
    for codec in &runtime.codecs {
        if codec.kind != CodecKind::Encoder {
            continue;
        }
        if runtime_codec_matches_selector(codec, encoder_id) {
            matched_names.insert(canonical_codec_family(&codec.name));
        }
    }
    matched_names.len() == 1 && targets.iter().any(|target| matched_names.contains(*target))
}

fn encoder_settings_shape(settings: &EncoderSettings) -> EncoderSettingsShape {
    match settings {
        EncoderSettings::Turbojpeg { .. } => EncoderSettingsShape::Turbojpeg,
        EncoderSettings::Mozjpeg { .. } => EncoderSettingsShape::Mozjpeg,
        EncoderSettings::FfmpegMjpeg { .. } => EncoderSettingsShape::FfmpegMjpeg,
        EncoderSettings::H264 { .. } => EncoderSettingsShape::H264,
        EncoderSettings::H265 { .. } => EncoderSettingsShape::H265,
    }
}

fn expected_encoder_settings_shape(selector: &str) -> Option<EncoderSettingsShape> {
    empty_encoder_settings_for_selector(Some(selector)).as_ref().map(encoder_settings_shape)
}

fn canonical_codec_family(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "mjpg" | "jpeg" => "mjpeg".to_string(),
        "avc" => "h264".to_string(),
        "hevc" => "h265".to_string(),
        other => other.to_string(),
    }
}

fn runtime_codec_matches_selector(codec: &helios_engine::ipc::StreamCodecCapability, selector: &str) -> bool {
    codec.implementation.eq_ignore_ascii_case(selector) || canonical_codec_family(&codec.name) == canonical_codec_family(selector)
}

fn runtime_codec_matches_input(codec: &helios_engine::ipc::StreamCodecCapability, input: styx::prelude::FourCc) -> bool {
    let target = String::from_utf8_lossy(&input.to_u32().to_le_bytes()).trim().to_ascii_uppercase();
    let codec_input = codec.input.trim().to_ascii_uppercase();
    let codec_fourcc = codec.fourcc.trim().to_ascii_uppercase();
    codec_input == target || codec_fourcc == target || codec_input == "ANY" || codec_fourcc == "ANY"
}

fn validate_file_backend_paths_present(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    if manifest.capture.backend != BackendKind::File {
        return;
    }

    let BackendHandle::File { paths, .. } = &manifest.capture.handle else {
        return;
    };

    if paths.is_empty() {
        issues.push(issue("/capture/handle/paths", "missing_paths", "file backend requires at least one non-empty path"));
    }
}

fn normalize_file_backend_paths(manifest: &mut StreamManifest, warnings: &mut Vec<ValidationWarning>) {
    if manifest.capture.backend != BackendKind::File {
        return;
    }

    let BackendHandle::File { paths, .. } = &mut manifest.capture.handle else {
        return;
    };

    let mut deduped = Vec::<PathBuf>::new();
    let mut seen = BTreeSet::<String>::new();
    for (index, path) in paths.iter().enumerate() {
        let raw = path.to_string_lossy().to_string();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            warnings.push(warning(format!("/capture/handle/paths/{index}"), "empty_path_removed", "removed empty file path"));
            continue;
        }
        if !seen.insert(trimmed.to_string()) {
            warnings.push(warning(format!("/capture/handle/paths/{index}"), "duplicate_path_removed", "removed duplicate file path entry"));
            continue;
        }
        if trimmed != raw {
            warnings.push(warning(format!("/capture/handle/paths/{index}"), "path_trimmed", "trimmed surrounding whitespace from file path"));
        }
        deduped.push(PathBuf::from(trimmed));
    }

    *paths = deduped;
}

fn file_replay_content_type(path: &Path) -> String {
    mime_guess::from_path(path).first_raw().unwrap_or("application/octet-stream").to_ascii_lowercase()
}

fn is_supported_file_replay_path(path: &Path) -> bool {
    let content_type = file_replay_content_type(path);
    if content_type.starts_with("image/") || content_type.starts_with("video/") {
        return true;
    }
    let ext = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
    FILE_BACKEND_SUPPORTED_EXTENSIONS.contains(&ext.as_str())
}

async fn validate_file_backend_media_paths(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    if manifest.capture.backend != BackendKind::File {
        return;
    }

    let BackendHandle::File { paths, .. } = &manifest.capture.handle else {
        issues.push(issue("/capture/handle", "invalid_file_handle", "file backend requires a file handle"));
        return;
    };

    for (index, path) in paths.iter().enumerate() {
        let pointer = format!("/capture/handle/paths/{index}");
        let metadata = match tokio::fs::metadata(path).await {
            Ok(meta) => meta,
            Err(_) => {
                issues.push(issue(pointer, "media_path_not_found", format!("media path not found: {}", path.display())));
                continue;
            }
        };
        if !metadata.is_file() {
            issues.push(issue(pointer, "media_path_not_file", format!("media path is not a file: {}", path.display())));
            continue;
        }
        if !is_supported_file_replay_path(path) {
            let content_type = file_replay_content_type(path);
            issues.push(issue(pointer, "unsupported_media_type", format!("unsupported media type `{content_type}` for file replay path: {}", path.display())));
        }
    }
}

fn normalize_reserved_pipeline_ids(manifest: &mut StreamManifest, warnings: &mut Vec<ValidationWarning>) {
    let before_pipelines = manifest.pipelines.len();
    manifest.pipelines.retain(|binding| binding.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID);
    if manifest.pipelines.len() != before_pipelines {
        warnings.push(warning("/pipelines", "reserved_pipeline_removed", "removed reserved calibration-mode pipeline binding"));
    }

    if manifest.active_pipeline_id == Some(CALIBRATION_MODE_PIPELINE_UUID) {
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        warnings.push(warning("/activePipelineId", "reserved_pipeline_removed", "removed reserved calibration-mode active pipeline selection"));
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for (index, slot) in layout.slots.iter_mut().enumerate() {
            if slot.pipeline_id == Some(CALIBRATION_MODE_PIPELINE_UUID) {
                slot.pipeline_id = None;
                slot.output_key = None;
                warnings.push(warning(format!("/pipelineLayout/slots/{index}"), "reserved_pipeline_removed", "removed reserved calibration-mode layout slot pipeline"));
            }
        }
    }

    let before_wires = manifest.pipeline_wires.len();
    manifest.pipeline_wires.retain(|wire| wire.from.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID && wire.to.pipeline_id != CALIBRATION_MODE_PIPELINE_UUID);
    if manifest.pipeline_wires.len() != before_wires {
        warnings.push(warning("/pipelineWires", "reserved_pipeline_removed", "removed wires referencing reserved calibration-mode pipeline"));
    }
}

fn validate_pipeline_layout(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    let Some(layout) = manifest.pipeline_layout.as_ref() else {
        return;
    };

    if layout.rows == 0 {
        issues.push(issue("/pipelineLayout/rows", "invalid_layout_rows", "pipeline layout rows must be at least 1"));
    }
    if layout.columns == 0 {
        issues.push(issue("/pipelineLayout/columns", "invalid_layout_columns", "pipeline layout columns must be at least 1"));
    }

    let known_ids = known_pipeline_ids(manifest);
    let mut seen_slots = BTreeSet::<(u8, u8)>::new();
    for (index, slot) in layout.slots.iter().enumerate() {
        if layout.rows > 0 && slot.row >= layout.rows {
            issues.push(issue(format!("/pipelineLayout/slots/{index}/row"), "slot_out_of_bounds", format!("slot row {} is outside layout row count {}", slot.row, layout.rows)));
        }
        if layout.columns > 0 && slot.column >= layout.columns {
            issues.push(issue(format!("/pipelineLayout/slots/{index}/column"), "slot_out_of_bounds", format!("slot column {} is outside layout column count {}", slot.column, layout.columns)));
        }
        if !seen_slots.insert((slot.row, slot.column)) {
            issues.push(issue(format!("/pipelineLayout/slots/{index}"), "duplicate_slot", format!("duplicate layout slot at row {}, column {}", slot.row, slot.column)));
        }

        if let Some(pipeline_id) = slot.pipeline_id
            && !known_ids.contains(&pipeline_id)
        {
            issues.push(issue(format!("/pipelineLayout/slots/{index}/pipelineId"), "unknown_pipeline", format!("layout references unknown pipeline id {pipeline_id}")));
        }
        if slot.pipeline_id.is_none() && slot.output_key.as_deref().is_some_and(|key| !key.trim().is_empty()) {
            issues.push(issue(format!("/pipelineLayout/slots/{index}/outputKey"), "dangling_output_key", "layout slot has output key but no pipeline id"));
        }
    }

    if let Some(active_pipeline_id) = manifest.active_pipeline_id
        && !known_ids.contains(&active_pipeline_id)
    {
        issues.push(issue("/activePipelineId", "unknown_pipeline", format!("active pipeline id {active_pipeline_id} is not present in stream bindings")));
    }
}

fn validate_pipeline_wires(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    let known_ids = known_pipeline_ids(manifest);
    for (index, wire) in manifest.pipeline_wires.iter().enumerate() {
        if !known_ids.contains(&wire.from.pipeline_id) {
            issues.push(issue(format!("/pipelineWires/{index}/from/pipelineId"), "unknown_pipeline", format!("wire source references unknown pipeline id {}", wire.from.pipeline_id)));
        }
        if !known_ids.contains(&wire.to.pipeline_id) {
            issues.push(issue(format!("/pipelineWires/{index}/to/pipelineId"), "unknown_pipeline", format!("wire destination references unknown pipeline id {}", wire.to.pipeline_id)));
        }
    }
}

async fn validate_pipeline_bindings(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    let mut seen_pipeline_ids = BTreeSet::<Uuid>::new();
    for (index, binding) in manifest.pipelines.iter().enumerate() {
        if binding.pipeline_graph.is_some() {
            issues.push(issue(format!("/pipelines/{index}/pipelineGraph"), "inline_pipeline_graph_forbidden", "inline pipeline graph payloads are not allowed; persist graph under /pipelines"));
        }
        if !seen_pipeline_ids.insert(binding.pipeline_id) {
            issues.push(issue(format!("/pipelines/{index}/pipelineId"), "duplicate_pipeline_binding", format!("duplicate pipeline binding for pipeline id {}", binding.pipeline_id)));
        }
    }

    let mut checked = BTreeSet::<Uuid>::new();
    for (index, binding) in manifest.pipelines.iter().enumerate() {
        let pipeline_id = binding.pipeline_id;
        if pipeline_id == RAW_PIPELINE_UUID {
            continue;
        }
        if !checked.insert(pipeline_id) {
            continue;
        }
        match pipelines::load_graph_document(pipeline_id).await {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                issues.push(issue(format!("/pipelines/{index}/pipelineId"), "pipeline_not_found", format!("pipeline {pipeline_id} is not persisted under /pipelines")));
            }
            Err(err) => {
                issues.push(issue(format!("/pipelines/{index}/pipelineId"), "pipeline_lookup_failed", format!("failed to resolve pipeline {pipeline_id}: {err}")));
            }
        }
    }
}

fn normalize_pipeline_output_fields(manifest: &mut StreamManifest, warnings: &mut Vec<ValidationWarning>) {
    trim_optional_output_field(&mut manifest.active_pipeline_output, "/activePipelineOutput", "active pipeline output", warnings);

    for (index, binding) in manifest.pipelines.iter_mut().enumerate() {
        trim_optional_output_field(&mut binding.pipeline_output, format!("/pipelines/{index}/pipelineOutput"), "pipeline output", warnings);
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for (index, slot) in layout.slots.iter_mut().enumerate() {
            trim_optional_output_field(&mut slot.output_key, format!("/pipelineLayout/slots/{index}/outputKey"), "layout output", warnings);
        }
    }

    for (index, wire) in manifest.pipeline_wires.iter_mut().enumerate() {
        trim_optional_output_field(&mut wire.from.output_key, format!("/pipelineWires/{index}/from/outputKey"), "wire output selector", warnings);
        trim_optional_output_field(&mut wire.from.port, format!("/pipelineWires/{index}/from/port"), "wire output port", warnings);
    }
}

fn normalize_file_stream_identity(manifest: &mut StreamManifest) {
    if manifest.capture.backend != styx::BackendKind::File {
        return;
    }

    let alias_missing = manifest.identity.alias.as_deref().map(str::trim).is_none_or(|value| value.is_empty());
    if alias_missing {
        let fallback = manifest.identity.id.map(|id| format!("media-replay-{id}")).unwrap_or_else(|| format!("media-replay-{}", Uuid::new_v4()));
        manifest.identity.alias = Some(fallback);
    }
}

fn capture_control_value_is_enabled(value: &helios_engine::capture::CaptureControlValue) -> bool {
    match value {
        helios_engine::capture::CaptureControlValue::Int(v) => *v != 0,
        helios_engine::capture::CaptureControlValue::Uint(v) => *v != 0,
        helios_engine::capture::CaptureControlValue::Float(v) => *v != 0.0,
        helios_engine::capture::CaptureControlValue::Bool(v) => *v,
        helios_engine::capture::CaptureControlValue::None => false,
    }
}

fn manifest_controls_require_tdn_output(manifest: &StreamManifest, descriptor: &helios_engine::capture::CaptureDescriptor) -> bool {
    manifest
        .capture
        .controls
        .iter()
        .any(|control| capture_control_value_is_enabled(&control.value) && descriptor.controls.iter().find(|meta| meta.id.0 == control.id).is_some_and(|meta| meta.metadata.requires_tdn_output))
}

fn normalize_capture_tdn_output_with_descriptor(manifest: &mut StreamManifest, descriptor: Option<&helios_engine::capture::CaptureDescriptor>) {
    if manifest.capture.backend != styx::BackendKind::Libcamera || !manifest.capture.enable_tdn_output {
        return;
    }
    let Some(descriptor) = descriptor else {
        return;
    };
    if !manifest_controls_require_tdn_output(manifest, descriptor) {
        manifest.capture.enable_tdn_output = false;
    }
}

fn normalize_capture_tdn_output(manifest: &mut StreamManifest) {
    let descriptor = helios_engine::capture::descriptor_for_config(&manifest.capture);
    normalize_capture_tdn_output_with_descriptor(manifest, descriptor.as_ref());
}

fn normalize_file_capture_manifest(manifest: &mut StreamManifest) {
    let styx::BackendHandle::File { paths, fps, loop_forever } = &manifest.capture.handle else {
        return;
    };

    let device = styx::capture_api::make_file_device("file-replay", paths.clone(), *fps, *loop_forever);
    let Some(backend) = device.backends.iter().find(|backend| backend.kind == styx::BackendKind::File) else {
        return;
    };

    let valid_control_ids: HashSet<u32> = backend.descriptor.controls.iter().map(|control| control.id.0).collect();
    if valid_control_ids.is_empty() {
        manifest.capture.controls.clear();
    } else {
        manifest.capture.controls.retain(|control| valid_control_ids.contains(&control.id));
    }

    let mode_is_valid = backend.descriptor.modes.iter().any(|mode| mode.id == manifest.capture.mode);
    if mode_is_valid {
        return;
    }

    let replacement_mode = backend.descriptor.modes.iter().find(|mode| mode.id.format == manifest.capture.mode.format).or_else(|| backend.descriptor.modes.first()).map(|mode| mode.id.clone());
    if let Some(mode) = replacement_mode {
        manifest.capture.mode = mode;
    }
}

fn trim_optional_output_field(value: &mut Option<String>, path: impl Into<String>, label: &'static str, warnings: &mut Vec<ValidationWarning>) {
    let path = path.into();
    let original = value.clone();
    let trimmed = value.take().and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    });
    if trimmed != original {
        warnings.push(warning(path, "output_trimmed", format!("{label} was trimmed")));
    }
    *value = trimmed;
}

fn validate_raw_pipeline_output_values(manifest: &StreamManifest, issues: &mut Vec<ValidationIssue>) {
    if manifest.active_pipeline_id == Some(RAW_PIPELINE_UUID) {
        validate_raw_output_value(&manifest.active_pipeline_output, "/activePipelineOutput", "active pipeline output", issues);
    }

    for (index, binding) in manifest.pipelines.iter().enumerate() {
        if binding.pipeline_id == RAW_PIPELINE_UUID {
            validate_raw_output_value(&binding.pipeline_output, format!("/pipelines/{index}/pipelineOutput"), "pipeline output", issues);
        }
    }

    if let Some(layout) = manifest.pipeline_layout.as_ref() {
        for (index, slot) in layout.slots.iter().enumerate() {
            if slot.pipeline_id == Some(RAW_PIPELINE_UUID) {
                validate_raw_output_value(&slot.output_key, format!("/pipelineLayout/slots/{index}/outputKey"), "layout output", issues);
            }
        }
    }

    for (index, wire) in manifest.pipeline_wires.iter().enumerate() {
        if wire.from.pipeline_id == RAW_PIPELINE_UUID {
            validate_raw_output_value(&wire.from.output_key, format!("/pipelineWires/{index}/from/outputKey"), "wire output selector", issues);
            validate_raw_output_value(&wire.from.port, format!("/pipelineWires/{index}/from/port"), "wire output port", issues);
        }
    }
}

fn validate_raw_output_value(value: &Option<String>, path: impl Into<String>, label: &'static str, issues: &mut Vec<ValidationIssue>) {
    if let Err(err) = normalize_pipeline_output_selection(value.as_deref(), Some(RAW_PIPELINE_UUID)) {
        issues.push(issue(path.into(), "unsupported_raw_output", format!("{label} is invalid: {err}")));
    }
}

fn known_pipeline_ids(manifest: &StreamManifest) -> BTreeSet<Uuid> {
    let mut out: BTreeSet<Uuid> = manifest.pipelines.iter().map(|binding| binding.pipeline_id).collect();
    out.insert(RAW_PIPELINE_UUID);
    out
}

fn backend_label(backend: BackendKind) -> &'static str {
    match backend {
        BackendKind::V4l2 => "v4l2",
        BackendKind::Libcamera => "libcamera",
        BackendKind::Virtual => "virtual",
        BackendKind::Netcam => "netcam",
        BackendKind::File => "file",
    }
}

fn handle_label(handle: &BackendHandle) -> &'static str {
    match handle {
        BackendHandle::V4l2 { .. } => "v4l2",
        BackendHandle::Libcamera { .. } => "libcamera",
        BackendHandle::Virtual => "virtual",
        BackendHandle::Netcam { .. } => "netcam",
        BackendHandle::File { .. } => "file",
    }
}

#[cfg(test)]
#[path = "validation_goldens.rs"]
mod validation_goldens;

#[cfg(test)]
mod tests {
    use super::*;
    use helios_engine::capture::{BackendHandle, BackendKind, CaptureConfig, CaptureControl, CaptureControlValue, ControlAssignment, ModeId};
    use helios_engine::identity::DeviceIdentity;
    use serde_json::json;
    use std::collections::BTreeMap;
    use styx::core::controls::{Access, ControlId, ControlKind, ControlMetadata, ControlValue};
    use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};

    #[test]
    fn stream_capabilities_publish_default_encoder_id() {
        let runtime = helios_engine::ipc::stream_runtime_capabilities().expect("runtime capabilities");
        let capabilities = stream_capabilities(&runtime);
        assert!(capabilities.defaults.default_encoder_id.as_deref().is_some_and(|value| !value.trim().is_empty()));
    }

    #[test]
    fn stream_capabilities_publish_decoder_defaults_by_capture_format() {
        let runtime = helios_engine::ipc::stream_runtime_capabilities().expect("runtime capabilities");
        let capabilities = stream_capabilities(&runtime);
        assert_eq!(capabilities.defaults.default_decoder_ids_by_capture_format.get("MJPG").map(String::as_str), Some("turbojpeg"));
        assert_eq!(capabilities.defaults.default_decoder_ids_by_capture_format.get("H264").map(String::as_str), Some("h264"));
        assert_eq!(capabilities.defaults.default_decoder_ids_by_capture_format.get("RG24").map(String::as_str), Some("passthrough"));
    }

    #[test]
    fn stream_capabilities_publish_effective_stream_defaults() {
        let runtime = helios_engine::ipc::stream_runtime_capabilities().expect("runtime capabilities");
        let capabilities = stream_capabilities(&runtime);
        assert_eq!(capabilities.defaults.pipeline_enabled_when_bindings_present, DEFAULT_STREAM_PIPELINE_ENABLED_WHEN_BINDINGS_PRESENT);
        assert_eq!(capabilities.defaults.default_encoder_enabled, default_encoder_enabled());
        assert_eq!(capabilities.defaults.default_decoder_enabled, default_decoder_enabled());
        assert_eq!(capabilities.defaults.default_host_buffer, default_host_buffer());
        assert_eq!(capabilities.defaults.default_preview_jpeg_quality, default_requested_preview_jpeg_quality_enabled());
        assert_eq!(capabilities.defaults.default_preview_jpeg_quality_when_encoder_disabled, default_requested_preview_jpeg_quality_disabled());
        assert_eq!(capabilities.defaults.default_recording_mode, default_recording_mode());
        assert_eq!(capabilities.defaults.default_start_on_boot, default_start_on_boot());
    }

    fn manifest_from_json(value: serde_json::Value) -> StreamManifest {
        serde_json::from_value(value).expect("valid stream manifest json")
    }

    fn base_manifest(path: &Path) -> StreamManifest {
        manifest_from_json(json!({
            "schema_version": helios_engine::ipc::CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
            "identity": { "alias": "cam0", "hardware_id": "cam0" },
            "capture": {
                "backend": "File",
                "handle": {
                    "type": "file",
                    "paths": [path.display().to_string()],
                    "fps": 30,
                    "loop_forever": false
                },
                "mode": {
                    "format": {
                        "code": "NV12",
                        "resolution": { "width": 1280, "height": 720 },
                        "color": "Unknown"
                    },
                    "interval": { "numerator": 1, "denominator": 30 }
                },
                "controls": []
            },
            "pipelines": [],
            "pipeline_layout": null,
            "pipeline_wires": [],
            "pipeline_host_inputs": {},
            "host_buffer": 2,
            "internal": false,
            "start_on_boot": false
        }))
    }

    async fn create_temp_media_file(ext: &str) -> PathBuf {
        let file = std::env::temp_dir().join(format!("helios-stream-validation-{}.{}", Uuid::new_v4(), ext));
        tokio::fs::write(&file, b"test").await.expect("create temporary media file");
        file
    }

    #[tokio::test]
    async fn rejects_empty_file_paths() {
        let file = create_temp_media_file("mp4").await;
        let mut manifest = base_manifest(&file);
        manifest.capture.handle = BackendHandle::File { paths: vec![PathBuf::from(""), PathBuf::from("   ")], fps: 30, loop_forever: false };

        let result = validate_stream_manifest(manifest).await;
        let err = result.expect_err("expected semantic validation failure");
        assert!(err.issues.iter().any(|issue| issue.code == "missing_paths"));
        let _ = tokio::fs::remove_file(&file).await;
    }

    #[tokio::test]
    async fn rejects_dangling_layout_pipeline_references() {
        let file = create_temp_media_file("mp4").await;
        let mut manifest = base_manifest(&file);
        manifest.pipeline_layout = Some(helios_engine::ipc::StreamPipelineLayout {
            rows: 1,
            columns: 1,
            slots: vec![helios_engine::ipc::StreamPipelineGridSlot { row: 0, column: 0, pipeline_id: Some(Uuid::new_v4()), output_key: Some("overlay".to_string()) }],
        });

        let result = validate_stream_manifest(manifest).await;
        let err = result.expect_err("expected dangling pipeline reference failure");
        assert!(err.issues.iter().any(|issue| issue.code == "unknown_pipeline"));
        let _ = tokio::fs::remove_file(&file).await;
    }

    #[tokio::test]
    async fn rejects_invalid_raw_output_alias() {
        let file = create_temp_media_file("mp4").await;
        let mut manifest = base_manifest(&file);
        manifest.pipeline_enabled = true;
        manifest.active_pipeline_id = Some(RAW_PIPELINE_UUID);
        manifest.active_pipeline_output = Some("FRAME".to_string());

        let err = validate_stream_manifest(manifest).await.expect_err("legacy RAW alias should fail");
        assert!(err.issues.iter().any(|issue| issue.code == "unsupported_raw_output"));
        let _ = tokio::fs::remove_file(&file).await;
    }

    #[tokio::test]
    async fn allows_missing_codec_selectors_when_not_explicitly_requested() {
        let file = create_temp_media_file("mp4").await;
        let manifest = base_manifest(&file);

        validate_stream_manifest(manifest).await.expect("validation should succeed");
        let _ = tokio::fs::remove_file(&file).await;
    }

    #[tokio::test]
    async fn rejects_unsupported_file_replay_media_type() {
        let file = create_temp_media_file("txt").await;
        let manifest = base_manifest(&file);

        let result = validate_stream_manifest(manifest).await;
        let err = result.expect_err("expected unsupported media type failure");
        assert!(err.issues.iter().any(|issue| issue.code == "unsupported_media_type"));
        let _ = tokio::fs::remove_file(&file).await;
    }

    #[tokio::test]
    async fn rejects_disabled_pipeline_with_pipeline_state() {
        let file = create_temp_media_file("mp4").await;
        let mut manifest = base_manifest(&file);
        let pipeline_id = Uuid::new_v4();
        manifest.pipeline_enabled = false;
        manifest.pipelines.push(helios_engine::ipc::StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: Some("overlay".to_string()), pipeline_patch: None });
        manifest.active_pipeline_id = Some(pipeline_id);

        let result = validate_stream_manifest(manifest).await;
        let err = result.expect_err("expected disabled pipeline state failure");
        assert!(err.issues.iter().any(|issue| issue.code == "pipeline_disabled_with_pipeline_state"));
        let _ = tokio::fs::remove_file(&file).await;
    }

    #[tokio::test]
    async fn rejects_unknown_encoder_selector() {
        let mut manifest = sample_libcamera_manifest();
        manifest.encoder = helios_engine::ipc::RequestedEncoderConfig::enabled(Some("definitely-missing-encoder".to_string()), None);

        let err = validate_stream_manifest(manifest).await.expect_err("expected unknown encoder failure");
        let issue = err.issues.iter().find(|issue| issue.code == "encoder_unavailable").expect("expected encoder availability issue");
        assert_eq!(issue.path, "/encoder/id");
        assert_eq!(issue.remediation.as_deref(), Some("Choose an available encoder selector for this stream, or disable encoding."));
    }

    #[tokio::test]
    async fn rejects_unknown_decoder_selector() {
        let mut manifest = sample_libcamera_manifest();
        manifest.decoder = helios_engine::ipc::RequestedDecoderConfig::enabled(Some("definitely-missing-decoder".to_string()), None);

        let err = validate_stream_manifest(manifest).await.expect_err("expected unknown decoder failure");
        let issue = err.issues.iter().find(|issue| issue.code == "decoder_unavailable").expect("expected decoder availability issue");
        assert_eq!(issue.path, "/decoder/id");
        assert_eq!(issue.remediation.as_deref(), Some("Choose a decoder selector that supports the selected capture format, or disable decoding."));
    }

    #[tokio::test]
    async fn rejects_encoder_settings_kind_mismatch() {
        let mut manifest = sample_libcamera_manifest();
        manifest.encoder = helios_engine::ipc::RequestedEncoderConfig::enabled(
            Some("turbojpeg".to_string()),
            Some(helios_engine::ipc::EncoderSettings::H264 { bitrate: Some(4_000_000), gop: None, framerate: None, thread_count: None, output_resolution: None }),
        );

        let err = validate_stream_manifest(manifest).await.expect_err("expected encoder settings kind mismatch");
        let issue = err.issues.iter().find(|issue| issue.code == "encoder_settings_kind_mismatch").expect("expected encoder settings kind issue");
        assert_eq!(issue.path, "/encoder/settings/kind");
        assert_eq!(issue.remediation.as_deref(), Some("Choose settings that match the selected encoder family, or switch the encoder selector to match the requested settings kind."));
    }

    #[tokio::test]
    async fn rejects_recording_mode_without_encoder() {
        let mut manifest = sample_libcamera_manifest();
        manifest.encoder = helios_engine::ipc::RequestedEncoderConfig::disabled();
        manifest.recording_mode = StreamRecordingMode::shadow_buffer(default_shadow_recording_codec());

        let err = validate_stream_manifest(manifest).await.expect_err("expected recording mode encoder requirement failure");
        let issue = err.issues.iter().find(|issue| issue.code == "recording_mode_requires_encoder").expect("expected recording mode encoder issue");
        assert_eq!(issue.path, "/recording_mode/state");
        assert_eq!(issue.remediation.as_deref(), Some("Enable the stream encoder before turning on shadow-buffer recording."));
    }

    #[tokio::test]
    async fn rejects_recording_mode_with_mismatched_encoder() {
        let mut manifest = sample_libcamera_manifest();
        manifest.encoder = helios_engine::ipc::RequestedEncoderConfig::enabled(Some("turbojpeg".to_string()), None);
        manifest.recording_mode = StreamRecordingMode::shadow_buffer(default_shadow_recording_codec());

        let err = validate_stream_manifest(manifest).await.expect_err("expected recording mode codec compatibility failure");
        let issue = err.issues.iter().find(|issue| issue.code == "recording_mode_codec_mismatch").expect("expected recording mode codec issue");
        assert_eq!(issue.path, "/recording_mode/codec");
        assert_eq!(issue.remediation.as_deref(), Some("Choose an encoder selector that matches the requested recording mode codec, or switch the recording mode codec to match the encoder."));
    }

    fn sample_libcamera_manifest() -> StreamManifest {
        let format = MediaFormat::new(FourCc::new(*b"NV12"), Resolution::new(1280, 800).unwrap(), ColorSpace::Srgb);
        StreamManifest {
            schema_version: helios_engine::ipc::CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
            identity: DeviceIdentity { id: Some(Uuid::new_v4()), alias: Some("camera".to_string()), hardware_id: Some("camera".to_string()) },
            capture: CaptureConfig {
                device_keys: vec!["camera".to_string()],
                device_identity: None,
                backend: BackendKind::Libcamera,
                handle: BackendHandle::Libcamera { id: "camera".to_string() },
                mode: ModeId { format, interval: None },
                target_fps: None,
                interval: None,
                controls: Vec::new(),
                enable_tdn_output: true,
            },
            host_buffer: 2,
            internal: false,
            pipeline_enabled: false,
            pipelines: Vec::new(),
            active_pipeline_id: None,
            active_pipeline_output: None,
            pipeline_layout: None,
            pipeline_wires: Vec::new(),
            pipeline_host_inputs: BTreeMap::new(),
            calibration: None,
            pose: None,
            encoder: helios_engine::ipc::RequestedEncoderConfig::default(),
            decoder: helios_engine::ipc::RequestedDecoderConfig::default(),
            preview_jpeg_quality: 30,
            recording_mode: default_recording_mode(),
            start_on_boot: false,
        }
    }

    fn sample_descriptor_with_noise_reduction(requires_tdn_output: bool) -> helios_engine::capture::CaptureDescriptor {
        helios_engine::capture::CaptureDescriptor {
            modes: Vec::new(),
            controls: vec![CaptureControl {
                id: ControlId(10002),
                name: "NoiseReductionMode".to_string(),
                kind: ControlKind::IntMenu,
                access: Access::ReadWrite,
                min: ControlValue::Int(0),
                max: ControlValue::Int(4),
                default: ControlValue::Int(0),
                step: None,
                menu: Some(vec![
                    "NoiseReductionModeOff".to_string(),
                    "NoiseReductionModeFast".to_string(),
                    "NoiseReductionModeHighQuality".to_string(),
                    "NoiseReductionModeMinimal".to_string(),
                    "NoiseReductionModeZSL".to_string(),
                ]),
                metadata: ControlMetadata { requires_tdn_output },
            }],
        }
    }

    #[test]
    fn normalize_stream_manifest_sets_missing_media_replay_alias() {
        let mut manifest = sample_libcamera_manifest();
        manifest.capture.backend = BackendKind::File;
        manifest.capture.handle = BackendHandle::File { paths: Vec::new(), fps: 30, loop_forever: false };
        manifest.capture.device_keys = vec!["media-file".to_string(), "video-0".to_string()];
        manifest.identity.alias = None;
        manifest.identity.hardware_id = Some("media-file".to_string());

        let normalized = normalize_stream_manifest(manifest);
        assert!(normalized.manifest.identity.alias.as_deref().is_some_and(|value| value.starts_with("media-replay-")));
        assert_eq!(normalized.manifest.capture.device_keys, vec!["media-file".to_string(), "video-0".to_string()]);
        assert_eq!(normalized.manifest.identity.hardware_id.as_deref(), Some("media-file"));
    }

    #[test]
    fn normalize_stream_manifest_clears_redundant_tdn_flag() {
        let mut manifest = sample_libcamera_manifest();
        manifest.capture.controls.push(ControlAssignment { id: 10002, value: CaptureControlValue::Int(1) });
        manifest.capture.enable_tdn_output = true;

        let descriptor = sample_descriptor_with_noise_reduction(false);
        normalize_capture_tdn_output_with_descriptor(&mut manifest, Some(&descriptor));

        assert!(!manifest.capture.enable_tdn_output);
    }

    #[test]
    fn normalize_stream_manifest_preserves_required_tdn_flag() {
        let mut manifest = sample_libcamera_manifest();
        manifest.capture.controls.push(ControlAssignment { id: 10002, value: CaptureControlValue::Int(1) });
        manifest.capture.enable_tdn_output = true;

        let descriptor = sample_descriptor_with_noise_reduction(true);
        normalize_capture_tdn_output_with_descriptor(&mut manifest, Some(&descriptor));

        assert!(manifest.capture.enable_tdn_output);
    }
}
