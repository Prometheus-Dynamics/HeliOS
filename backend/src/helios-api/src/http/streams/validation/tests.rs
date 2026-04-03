use super::*;
use helios_engine::capture::{BackendHandle, BackendKind, CaptureConfig, CaptureControl, CaptureControlValue, ControlAssignment, ModeId};
use helios_engine::identity::DeviceIdentity;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use styx::core::controls::{Access, ControlId, ControlKind, ControlMetadata, ControlValue};
use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};
use uuid::Uuid;

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

async fn create_temp_media_file(ext: &str) -> std::path::PathBuf {
    let file = std::env::temp_dir().join(format!("helios-stream-validation-{}.{}", Uuid::new_v4(), ext));
    tokio::fs::write(&file, b"test").await.expect("create temporary media file");
    file
}

#[tokio::test]
async fn rejects_empty_file_paths() {
    let file = create_temp_media_file("mp4").await;
    let mut manifest = base_manifest(&file);
    manifest.capture.handle = BackendHandle::File { paths: vec![std::path::PathBuf::from(""), std::path::PathBuf::from("   ")], fps: 30, loop_forever: false };

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
