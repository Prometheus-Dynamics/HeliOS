use super::*;
use crate::http::streams::lifecycle::codec_inventory_from_runtime;
use helios_engine::capture::{BackendHandle, BackendKind, CaptureConfig, ModeId};
use helios_engine::identity::DeviceIdentity;
use helios_engine::ipc::{
    CURRENT_STREAM_CONFIG_SCHEMA_VERSION, EncoderSettings, PoseRotation, PoseVector, RequestedDecoderConfig, RequestedEncoderConfig, RigPose, StreamCodecCapability, StreamCodecTunables,
    StreamManifest, StreamPipelineGridSlot, StreamRecordingMode, StreamRuntimeCapabilities, default_shadow_recording_codec, stream_runtime_capabilities,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use styx::codec::CodecKind;
use styx::core::format::Interval;
use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};
use uuid::Uuid;

const GOLDEN_ENV: &str = "UPDATE_STREAM_CAPABILITY_GOLDENS";
const GOLDEN_PATH: &str = "../../../testdata/stream_capability_compatibility_goldens.json";

#[derive(Debug, Clone, Copy)]
enum RuntimeProfile {
    CurrentRuntime,
    FocusedRuntime,
    NoCodecs,
}

impl RuntimeProfile {
    fn name(self) -> &'static str {
        match self {
            Self::CurrentRuntime => "current_runtime",
            Self::FocusedRuntime => "focused_runtime",
            Self::NoCodecs => "no_codecs",
        }
    }

    fn runtime(self) -> StreamRuntimeCapabilities {
        match self {
            Self::CurrentRuntime => stream_runtime_capabilities().expect("runtime capabilities"),
            Self::FocusedRuntime => focused_runtime_capabilities(),
            Self::NoCodecs => StreamRuntimeCapabilities { codecs: Vec::new(), default_encoder_id: None, default_decoder_ids_by_capture_format: BTreeMap::new() },
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct StreamCapabilityGoldenFile {
    capability_cases: Vec<CapabilityGoldenSnapshot>,
    codec_cases: Vec<CodecGoldenSnapshot>,
    validation_cases: Vec<ValidationGoldenSnapshot>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CapabilityGoldenSnapshot {
    name: String,
    profile: String,
    response: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct CodecGoldenSnapshot {
    name: String,
    profile: String,
    response: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct ValidationGoldenSnapshot {
    name: String,
    profile: String,
    manifest: serde_json::Value,
    outcome: serde_json::Value,
}

#[derive(Debug, Clone, Copy)]
enum ValidationCaseKind {
    ValidRawPipeline,
    ValidPoseNoPipeline,
    InvalidUnknownEncoder,
    InvalidUnknownDecoder,
    InvalidRecordingRequiresEncoder,
    InvalidRecordingCodecMismatch,
    InvalidEncoderSettingsKindMismatch,
    InvalidPipelineDisabledWithState,
}

#[derive(Debug, Clone, Copy)]
struct ValidationCaseDef {
    name: &'static str,
    profile: RuntimeProfile,
    kind: ValidationCaseKind,
}

impl ValidationCaseDef {
    fn manifest(self) -> StreamManifest {
        match self.kind {
            ValidationCaseKind::ValidRawPipeline => {
                let mut manifest = sample_libcamera_manifest(FourCc::new(*b"RG24"));
                manifest.encoder = RequestedEncoderConfig::enabled(Some("turbojpeg".to_string()), Some(EncoderSettings::Turbojpeg { quality: Some(72) }));
                manifest.decoder = RequestedDecoderConfig::disabled();
                manifest.pipeline_enabled = true;
                manifest.pipelines =
                    vec![helios_engine::ipc::StreamPipelineBinding { pipeline_id: RAW_PIPELINE_UUID, pipeline_graph: None, pipeline_output: Some("raw".to_string()), pipeline_patch: None }];
                manifest.active_pipeline_id = Some(RAW_PIPELINE_UUID);
                manifest.active_pipeline_output = Some("raw".to_string());
                manifest.pipeline_layout = Some(helios_engine::ipc::StreamPipelineLayout {
                    rows: 1,
                    columns: 1,
                    slots: vec![StreamPipelineGridSlot { row: 0, column: 0, pipeline_id: Some(RAW_PIPELINE_UUID), output_key: Some("raw".to_string()) }],
                });
                manifest.preview_jpeg_quality = 72;
                manifest
            }
            ValidationCaseKind::ValidPoseNoPipeline => {
                let mut manifest = sample_libcamera_manifest(FourCc::new(*b"RG24"));
                manifest.host_buffer = 9;
                manifest.preview_jpeg_quality = 44;
                manifest.capture.interval = Some(Interval { numerator: NonZeroU32::new(1).expect("non-zero numerator"), denominator: NonZeroU32::new(15).expect("non-zero denominator") });
                manifest.encoder = RequestedEncoderConfig::disabled();
                manifest.decoder = RequestedDecoderConfig::disabled();
                manifest.pose = Some(RigPose {
                    translation: PoseVector { x: 1.0, y: 2.0, z: 3.0 },
                    rotation: PoseRotation { roll: 4.0, pitch: 5.0, yaw: 6.0 },
                    updated_at: Some("2026-04-01T00:00:00Z".to_string()),
                });
                manifest.start_on_boot = true;
                manifest
            }
            ValidationCaseKind::InvalidUnknownEncoder => {
                let mut manifest = sample_libcamera_manifest(FourCc::new(*b"RG24"));
                manifest.encoder = RequestedEncoderConfig::enabled(Some("missing-encoder".to_string()), None);
                manifest
            }
            ValidationCaseKind::InvalidUnknownDecoder => {
                let mut manifest = sample_libcamera_manifest(FourCc::new(*b"MJPG"));
                manifest.encoder = RequestedEncoderConfig::disabled();
                manifest.decoder = RequestedDecoderConfig::enabled(Some("missing-decoder".to_string()), None);
                manifest
            }
            ValidationCaseKind::InvalidRecordingRequiresEncoder => {
                let mut manifest = sample_libcamera_manifest(FourCc::new(*b"RG24"));
                manifest.encoder = RequestedEncoderConfig::disabled();
                manifest.recording_mode = StreamRecordingMode::shadow_buffer(default_shadow_recording_codec());
                manifest
            }
            ValidationCaseKind::InvalidRecordingCodecMismatch => {
                let mut manifest = sample_libcamera_manifest(FourCc::new(*b"RG24"));
                manifest.encoder = RequestedEncoderConfig::enabled(Some("turbojpeg".to_string()), None);
                manifest.recording_mode = StreamRecordingMode::shadow_buffer(default_shadow_recording_codec());
                manifest
            }
            ValidationCaseKind::InvalidEncoderSettingsKindMismatch => {
                let mut manifest = sample_libcamera_manifest(FourCc::new(*b"RG24"));
                manifest.encoder = RequestedEncoderConfig::enabled(
                    Some("turbojpeg".to_string()),
                    Some(EncoderSettings::H264 { bitrate: Some(4_000_000), gop: None, framerate: None, thread_count: None, output_resolution: None }),
                );
                manifest
            }
            ValidationCaseKind::InvalidPipelineDisabledWithState => {
                let mut manifest = sample_libcamera_manifest(FourCc::new(*b"RG24"));
                manifest.pipeline_enabled = false;
                manifest.active_pipeline_id = Some(RAW_PIPELINE_UUID);
                manifest.active_pipeline_output = Some("raw".to_string());
                manifest.pipeline_layout = Some(helios_engine::ipc::StreamPipelineLayout {
                    rows: 1,
                    columns: 1,
                    slots: vec![StreamPipelineGridSlot { row: 0, column: 0, pipeline_id: Some(RAW_PIPELINE_UUID), output_key: Some("raw".to_string()) }],
                });
                manifest.pipelines =
                    vec![helios_engine::ipc::StreamPipelineBinding { pipeline_id: RAW_PIPELINE_UUID, pipeline_graph: None, pipeline_output: Some("raw".to_string()), pipeline_patch: None }];
                manifest
            }
        }
    }
}

fn validation_case_defs() -> [ValidationCaseDef; 8] {
    [
        ValidationCaseDef { name: "valid_raw_pipeline", profile: RuntimeProfile::FocusedRuntime, kind: ValidationCaseKind::ValidRawPipeline },
        ValidationCaseDef { name: "valid_pose_without_pipeline", profile: RuntimeProfile::FocusedRuntime, kind: ValidationCaseKind::ValidPoseNoPipeline },
        ValidationCaseDef { name: "invalid_unknown_encoder", profile: RuntimeProfile::FocusedRuntime, kind: ValidationCaseKind::InvalidUnknownEncoder },
        ValidationCaseDef { name: "invalid_unknown_decoder", profile: RuntimeProfile::FocusedRuntime, kind: ValidationCaseKind::InvalidUnknownDecoder },
        ValidationCaseDef { name: "invalid_recording_requires_encoder", profile: RuntimeProfile::FocusedRuntime, kind: ValidationCaseKind::InvalidRecordingRequiresEncoder },
        ValidationCaseDef { name: "invalid_recording_codec_mismatch", profile: RuntimeProfile::FocusedRuntime, kind: ValidationCaseKind::InvalidRecordingCodecMismatch },
        ValidationCaseDef { name: "invalid_encoder_settings_kind_mismatch", profile: RuntimeProfile::FocusedRuntime, kind: ValidationCaseKind::InvalidEncoderSettingsKindMismatch },
        ValidationCaseDef { name: "invalid_pipeline_disabled_with_state", profile: RuntimeProfile::FocusedRuntime, kind: ValidationCaseKind::InvalidPipelineDisabledWithState },
    ]
}

fn focused_runtime_capabilities() -> StreamRuntimeCapabilities {
    StreamRuntimeCapabilities {
        codecs: vec![
            codec_capability(CodecKind::Decoder, "MJPG", "TurboJPEG Decoder", "turbojpeg", "MJPG", "RG24", None),
            codec_capability(CodecKind::Decoder, "H264", "H264 Decoder", "h264", "H264", "RG24", None),
            codec_capability(CodecKind::Decoder, "RG24", "Passthrough Decoder", "passthrough", "RG24", "RG24", None),
            codec_capability(
                CodecKind::Encoder,
                "RG24",
                "TurboJPEG Encoder",
                "turbojpeg",
                "RG24",
                "MJPG",
                Some(StreamCodecTunables { encoder_settings: Some(EncoderSettings::Turbojpeg { quality: Some(85) }) }),
            ),
            codec_capability(
                CodecKind::Encoder,
                "RG24",
                "H264 Encoder",
                "h264",
                "RG24",
                "H264",
                Some(StreamCodecTunables { encoder_settings: Some(EncoderSettings::H264 { bitrate: Some(4_000_000), gop: None, framerate: None, thread_count: None, output_resolution: None }) }),
            ),
        ],
        default_encoder_id: Some("turbojpeg".to_string()),
        default_decoder_ids_by_capture_format: BTreeMap::from([
            ("ANY".to_string(), "passthrough".to_string()),
            ("H264".to_string(), "h264".to_string()),
            ("MJPG".to_string(), "turbojpeg".to_string()),
            ("RG24".to_string(), "passthrough".to_string()),
        ]),
    }
}

fn codec_capability(kind: CodecKind, fourcc: &str, name: &str, implementation: &str, input: &str, output: &str, tunables: Option<StreamCodecTunables>) -> StreamCodecCapability {
    StreamCodecCapability { kind, fourcc: fourcc.to_string(), name: name.to_string(), implementation: implementation.to_string(), input: input.to_string(), output: output.to_string(), tunables }
}

fn sample_libcamera_manifest(fourcc: FourCc) -> StreamManifest {
    let format = MediaFormat::new(fourcc, Resolution::new(1280, 800).unwrap(), ColorSpace::Srgb);
    StreamManifest {
        schema_version: CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
        identity: DeviceIdentity { id: Some(Uuid::nil()), alias: Some("golden-camera".to_string()), hardware_id: Some("golden-camera".to_string()) },
        capture: CaptureConfig {
            device_keys: vec!["golden-camera".to_string()],
            device_identity: None,
            backend: BackendKind::Libcamera,
            handle: BackendHandle::Libcamera { id: "golden-camera".to_string() },
            mode: ModeId { format, interval: None },
            target_fps: None,
            interval: None,
            controls: Vec::new(),
            enable_tdn_output: false,
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
        encoder: RequestedEncoderConfig::enabled(Some("turbojpeg".to_string()), None),
        decoder: RequestedDecoderConfig::disabled(),
        preview_jpeg_quality: 30,
        recording_mode: default_recording_mode(),
        start_on_boot: false,
    }
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN_PATH)
}

fn update_goldens() -> bool {
    std::env::var_os(GOLDEN_ENV).is_some()
}

fn load_goldens(path: &Path) -> StreamCapabilityGoldenFile {
    let bytes = fs::read(path).unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()))
}

fn write_goldens(path: &Path, goldens: &StreamCapabilityGoldenFile) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|err| panic!("failed to create {}: {err}", parent.display()));
    }
    let body = serde_json::to_vec_pretty(goldens).expect("serialize goldens");
    fs::write(path, body).unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
}

fn capability_case(name: &str, profile: RuntimeProfile) -> CapabilityGoldenSnapshot {
    let runtime = profile.runtime();
    CapabilityGoldenSnapshot { name: name.to_string(), profile: profile.name().to_string(), response: serde_json::to_value(stream_capabilities(&runtime)).expect("serialize capability response") }
}

fn codec_case(name: &str, profile: RuntimeProfile) -> CodecGoldenSnapshot {
    let runtime = profile.runtime();
    CodecGoldenSnapshot { name: name.to_string(), profile: profile.name().to_string(), response: serde_json::to_value(codec_inventory_from_runtime(runtime)).expect("serialize codec inventory") }
}

async fn validation_case(def: ValidationCaseDef) -> ValidationGoldenSnapshot {
    let manifest = def.manifest();
    let runtime = def.profile.runtime();
    let outcome = match validate_stream_manifest_with_runtime(manifest.clone(), &runtime).await {
        Ok(result) => serde_json::json!({
            "status": "valid",
            "manifest": result.manifest,
            "resolved": result.resolved,
            "warnings": result.warnings,
        }),
        Err(err) => serde_json::json!({
            "status": "invalid",
            "issues": err.issues,
            "warnings": err.warnings,
        }),
    };
    ValidationGoldenSnapshot { name: def.name.to_string(), profile: def.profile.name().to_string(), manifest: serde_json::to_value(manifest).expect("serialize manifest"), outcome }
}

async fn generate_goldens() -> StreamCapabilityGoldenFile {
    let capability_cases = vec![
        capability_case("current_runtime_defaults_and_constraints", RuntimeProfile::CurrentRuntime),
        capability_case("focused_runtime_defaults_and_constraints", RuntimeProfile::FocusedRuntime),
        capability_case("no_codec_defaults_and_constraints", RuntimeProfile::NoCodecs),
    ];
    let codec_cases = vec![codec_case("current_runtime_codec_inventory", RuntimeProfile::CurrentRuntime), codec_case("focused_runtime_codec_inventory", RuntimeProfile::FocusedRuntime)];
    let mut validation_cases = Vec::new();
    for def in validation_case_defs() {
        validation_cases.push(validation_case(def).await);
    }
    StreamCapabilityGoldenFile { capability_cases, codec_cases, validation_cases }
}

#[tokio::test]
async fn stream_capability_compatibility_goldens_are_in_sync() {
    let path = golden_path();
    let actual = generate_goldens().await;

    if update_goldens() || !path.exists() {
        write_goldens(&path, &actual);
        return;
    }

    let expected = load_goldens(&path);
    let expected_json = serde_json::to_string_pretty(&expected).expect("serialize expected goldens");
    let actual_json = serde_json::to_string_pretty(&actual).expect("serialize actual goldens");
    assert_eq!(
        actual_json,
        expected_json,
        "stream capability/compatibility goldens changed; review the diff in {} and rerun `python3 tools/update_stream_capability_goldens.py` if the change is intentional",
        path.display()
    );
}
