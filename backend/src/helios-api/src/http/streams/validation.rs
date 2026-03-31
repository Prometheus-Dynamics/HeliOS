use helios_engine::ipc::StreamManifest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use styx::{BackendHandle, BackendKind};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::pipelines;
use crate::http::validation::{ValidationIssue, ValidationWarning, issue, warning};

use super::util;
use super::{CALIBRATION_MODE_PIPELINE_UUID, RAW_PIPELINE_UUID};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamValidateResponse {
    pub manifest: StreamManifest,
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
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone)]
pub struct StreamValidationError {
    pub issues: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationWarning>,
}

const FILE_BACKEND_SUPPORTED_EXTENSIONS: &[&str] =
    &["bmp", "gif", "heic", "heif", "jpeg", "jpg", "png", "tif", "tiff", "webp", "h264", "avc", "h265", "hevc", "m4v", "mjpeg", "mjpg", "mov", "mp4", "mpe", "mpeg", "mpg", "webm", "wmv", "y4m"];

pub fn stream_capabilities() -> StreamCapabilitiesResponse {
    StreamCapabilitiesResponse {
        raw_pipeline_id: RAW_PIPELINE_UUID,
        calibration_mode_pipeline_id: CALIBRATION_MODE_PIPELINE_UUID,
        defaults: StreamValidationDefaults {
            raw_output: "raw".to_string(),
            undistorted_output: "undistorted".to_string(),
            pipeline_enabled_when_bindings_present: true,
            default_encoder_id: util::default_stream_encoder_selector(),
            default_decoder_ids_by_capture_format: util::default_decoder_ids_by_capture_format(),
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

pub async fn validate_stream_manifest(mut manifest: StreamManifest) -> Result<StreamValidationResult, StreamValidationError> {
    let mut issues = Vec::<ValidationIssue>::new();
    let mut warnings = Vec::<ValidationWarning>::new();

    validate_backend_and_handle(&manifest, &mut issues);
    sanitize_file_backend_paths(&mut manifest, &mut issues, &mut warnings);
    validate_file_backend_media_paths(&manifest, &mut issues).await;
    sanitize_reserved_pipeline_ids(&mut manifest, &mut warnings);
    validate_pipeline_layout(&manifest, &mut issues);
    validate_pipeline_wires(&manifest, &mut issues);
    validate_pipeline_bindings(&manifest, &mut issues).await;
    canonicalize_pipeline_output_aliases(&mut manifest, &mut warnings);

    if issues.is_empty() {
        util::normalize_pipeline_manifest(&mut manifest);
        Ok(StreamValidationResult { manifest, warnings })
    } else {
        Err(StreamValidationError { issues, warnings })
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

fn sanitize_file_backend_paths(manifest: &mut StreamManifest, issues: &mut Vec<ValidationIssue>, warnings: &mut Vec<ValidationWarning>) {
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
    if paths.is_empty() {
        issues.push(issue("/capture/handle/paths", "missing_paths", "file backend requires at least one non-empty path"));
    }
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

fn sanitize_reserved_pipeline_ids(manifest: &mut StreamManifest, warnings: &mut Vec<ValidationWarning>) {
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

fn canonicalize_pipeline_output_aliases(manifest: &mut StreamManifest, warnings: &mut Vec<ValidationWarning>) {
    if manifest.active_pipeline_id == Some(RAW_PIPELINE_UUID) {
        canonicalize_raw_output_field(&mut manifest.active_pipeline_output, "/activePipelineOutput", "active pipeline output", warnings);
    } else {
        trim_optional_output_field(&mut manifest.active_pipeline_output, "/activePipelineOutput", "active pipeline output", warnings);
    }

    for (index, binding) in manifest.pipelines.iter_mut().enumerate() {
        if binding.pipeline_id == RAW_PIPELINE_UUID {
            canonicalize_raw_output_field(&mut binding.pipeline_output, format!("/pipelines/{index}/pipelineOutput"), "pipeline output", warnings);
        } else {
            trim_optional_output_field(&mut binding.pipeline_output, format!("/pipelines/{index}/pipelineOutput"), "pipeline output", warnings);
        }
    }

    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for (index, slot) in layout.slots.iter_mut().enumerate() {
            if slot.pipeline_id == Some(RAW_PIPELINE_UUID) {
                canonicalize_raw_output_field(&mut slot.output_key, format!("/pipelineLayout/slots/{index}/outputKey"), "layout output", warnings);
            } else {
                trim_optional_output_field(&mut slot.output_key, format!("/pipelineLayout/slots/{index}/outputKey"), "layout output", warnings);
            }
        }
    }

    for (index, wire) in manifest.pipeline_wires.iter_mut().enumerate() {
        if wire.from.pipeline_id == RAW_PIPELINE_UUID {
            canonicalize_raw_output_field(&mut wire.from.output_key, format!("/pipelineWires/{index}/from/outputKey"), "wire output selector", warnings);
        } else {
            trim_optional_output_field(&mut wire.from.output_key, format!("/pipelineWires/{index}/from/outputKey"), "wire output selector", warnings);
        }
    }
}

fn canonicalize_raw_output_field(value: &mut Option<String>, path: impl Into<String>, label: &'static str, warnings: &mut Vec<ValidationWarning>) {
    let path = path.into();
    let original = value.clone();
    let canonical = canonicalize_raw_output(value.take());
    if canonical != original {
        warnings.push(warning(path, "raw_output_canonicalized", format!("{label} was canonicalized to a supported RAW output value")));
    }
    *value = canonical;
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

fn canonicalize_raw_output(value: Option<String>) -> Option<String> {
    let normalized = value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    })?;
    if normalized.eq_ignore_ascii_case("undistorted") { Some("undistorted".to_string()) } else { Some("raw".to_string()) }
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
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stream_capabilities_publish_default_encoder_id() {
        let capabilities = stream_capabilities();
        assert!(capabilities.defaults.default_encoder_id.as_deref().is_some_and(|value| !value.trim().is_empty()));
    }

    #[test]
    fn stream_capabilities_publish_decoder_defaults_by_capture_format() {
        let capabilities = stream_capabilities();
        assert_eq!(capabilities.defaults.default_decoder_ids_by_capture_format.get("MJPG").map(String::as_str), Some("turbojpeg"));
        assert_eq!(capabilities.defaults.default_decoder_ids_by_capture_format.get("H264").map(String::as_str), Some("h264"));
        assert_eq!(capabilities.defaults.default_decoder_ids_by_capture_format.get("RG24").map(String::as_str), Some("passthrough"));
    }

    fn manifest_from_json(value: serde_json::Value) -> StreamManifest {
        serde_json::from_value(value).expect("valid stream manifest json")
    }

    fn base_manifest(path: &Path) -> StreamManifest {
        manifest_from_json(json!({
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
            "pipelineLayout": null,
            "pipelineWires": [],
            "pipelineHostInputs": {},
            "hostBuffer": 2,
            "internal": false,
            "shadowRecorderEnabled": false,
            "startOnBoot": false
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
    async fn warns_for_raw_output_alias_normalization() {
        let file = create_temp_media_file("mp4").await;
        let mut manifest = base_manifest(&file);
        manifest.active_pipeline_id = Some(RAW_PIPELINE_UUID);
        manifest.active_pipeline_output = Some("FRAME".to_string());

        let result = validate_stream_manifest(manifest).await.expect("validation should succeed");
        assert_eq!(result.manifest.active_pipeline_output.as_deref(), Some("raw"));
        assert!(result.warnings.iter().any(|warning| warning.code == "raw_output_canonicalized"));
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
}
