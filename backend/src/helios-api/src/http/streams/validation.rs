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

mod checks;
mod normalize;

use checks::*;
use normalize::*;

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

#[cfg(test)]
#[path = "validation_goldens.rs"]
mod validation_goldens;

#[cfg(test)]
mod tests;
