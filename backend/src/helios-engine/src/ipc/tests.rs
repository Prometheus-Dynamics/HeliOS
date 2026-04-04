use super::*;
use lib_ipc::archive::{DecodeStrategy, DecodeValidator, TransportEncode};
use lib_ipc::types::CommandId;
use lib_ipc::wire::{FrameFlags, ServiceKind, StreamKind};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::BTreeSet;
use styx::codec::{CodecKind, CodecRegistry};
use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};

fn sample_manifest() -> StreamManifest {
    let identity = crate::identity::DeviceIdentity { id: None, alias: None, hardware_id: None };
    let fmt = MediaFormat::new(FourCc::new(*b"RGB3"), Resolution::new(1, 1).unwrap(), ColorSpace::Srgb);
    let capture = crate::capture::CaptureConfig {
        device_keys: vec![],
        device_identity: None,
        backend: crate::capture::BackendKind::Virtual,
        handle: crate::capture::BackendHandle::Virtual,
        mode: crate::capture::ModeId { format: fmt, interval: None },
        target_fps: None,
        interval: None,
        controls: vec![],
        enable_tdn_output: false,
    };
    StreamManifest {
        schema_version: CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
        identity,
        capture,
        host_buffer: super::default_host_buffer(),
        internal: false,
        pipeline_enabled: false,
        pipelines: Vec::new(),
        active_pipeline_id: None,
        active_pipeline_output: None,
        pipeline_layout: None,
        pipeline_wires: Vec::new(),
        pipeline_host_inputs: std::collections::BTreeMap::new(),
        calibration: None,
        pose: None,
        encoder: RequestedEncoderConfig::default(),
        decoder: RequestedDecoderConfig::default(),
        preview_jpeg_quality: 30,
        recording_mode: StreamRecordingMode::shadow_buffer(default_shadow_recording_codec()),
        start_on_boot: false,
    }
}

fn assert_json_round_trip<T>(value: &T) -> T
where
    T: Serialize + DeserializeOwned,
{
    let encoded = serde_json::to_vec(value).expect("encode json");
    serde_json::from_slice(&encoded).expect("decode json")
}

fn assert_ipc_round_trip<T>(service: ServiceKind, stream: StreamKind, request_id: CommandId, value: &T) -> T
where
    T: TransportEncode + rkyv::Archive,
    T::Archived: for<'a> rkyv::bytecheck::CheckBytes<DecodeValidator<'a>> + rkyv::Deserialize<T, DecodeStrategy>,
{
    let frame = lib_ipc::frame::Frame::encode_payload(service, stream, request_id, FrameFlags::empty(), value).expect("encode ipc payload");
    frame.decode_payload().expect("decode ipc payload")
}

#[test]
fn start_command_round_trips_over_ipc_frame() {
    let base_manifest = sample_manifest();

    let decoded_identity: crate::identity::DeviceIdentity = assert_json_round_trip(&base_manifest.identity);
    assert_eq!(decoded_identity.alias, base_manifest.identity.alias);

    let capture = &base_manifest.capture;
    let decoded_handle: crate::capture::BackendHandle = assert_json_round_trip(&capture.handle);
    assert!(matches!(decoded_handle, crate::capture::BackendHandle::Virtual));
    let decoded_backend: crate::capture::BackendKind = assert_json_round_trip(&capture.backend);
    assert_eq!(decoded_backend, crate::capture::BackendKind::Virtual);
    let decoded_mode: crate::capture::ModeId = assert_json_round_trip(&capture.mode);
    assert_eq!(decoded_mode, capture.mode);

    let decoded_capture: crate::capture::CaptureConfig = assert_json_round_trip(capture);
    assert_eq!(decoded_capture.backend, capture.backend);

    let decoded_manifest: StreamManifest = assert_json_round_trip(&base_manifest);
    assert_eq!(decoded_manifest.capture.backend, base_manifest.capture.backend);

    let command = EngineCommand::Start { command_id: CommandId::new(), manifest: Box::new(base_manifest.clone().resolve()) };
    let decoded: EngineCommand = assert_ipc_round_trip(ServiceKind::Engine, StreamKind::Request, CommandId::new(), &command);
    if let EngineCommand::Start { manifest: round_trip, .. } = decoded {
        assert_eq!(round_trip.capture.backend, base_manifest.capture.backend);
    } else {
        panic!("unexpected command after decode");
    }
}

#[test]
fn stream_list_event_round_trips_over_ipc_frame() {
    let (_, descriptor) = crate::capture::default_virtual_device().backends.into_iter().next().map(|b| b.descriptor).map(|d| ((), d)).unwrap();
    let summary = StreamSummary { stream_id: uuid::Uuid::new_v4(), descriptor, manifest: sample_manifest().resolve(), status: StreamStatus::default(), runtime: StreamRuntimeState::default() };
    let event = EngineEvent::StreamList { command_id: CommandId::new(), streams: vec![summary.clone()] };
    let decoded: EngineEvent = assert_ipc_round_trip(ServiceKind::Engine, StreamKind::Event, CommandId::new(), &event);
    match decoded {
        EngineEvent::StreamList { streams, .. } => {
            assert_eq!(streams.len(), 1);
            assert_eq!(streams[0].descriptor.modes.len(), summary.descriptor.modes.len());
            assert_eq!(streams[0].runtime.capture.state, StreamCaptureState::Stopped);
        }
        other => panic!("unexpected event after decode: {:?}", other),
    }
}

#[test]
fn runtime_status_uses_capture_and_recording_state() {
    let mut runtime = StreamRuntimeState::default();
    runtime.capture.state = StreamCaptureState::Disabled;
    runtime.capture.started_at_ms = Some(10);
    runtime.capture.disabled_since_ms = Some(20);
    runtime.capture.disabled_reason = Some("graph fault".to_string());
    runtime.recording.state = StreamRecordingState::Active;
    runtime.recording.started_at_ms = Some(30);

    let status = runtime.status();
    assert_eq!(status.state, StreamState::Disabled);
    assert_eq!(status.started_at_ms, Some(10));
    assert_eq!(status.disabled_since_ms, Some(20));
    assert_eq!(status.disabled_reason.as_deref(), Some("graph fault"));
    assert!(status.recording_active);
    assert_eq!(status.recording_since_ms, Some(30));
}

#[test]
fn runtime_state_round_trips_over_json() {
    let runtime = StreamRuntimeState::default();

    let _: StreamCaptureRuntimeState = assert_json_round_trip(&runtime.capture);

    let _: StreamCodecChainRuntimeState = assert_json_round_trip(&runtime.codecs);

    let _: StreamDemandRuntimeState = assert_json_round_trip(&runtime.demand);

    let _: StreamRecordingRuntimeState = assert_json_round_trip(&runtime.recording);

    let _: StreamPipelineRuntimeState = assert_json_round_trip(&runtime.pipeline);

    let decoded: StreamRuntimeState = assert_json_round_trip(&runtime);
    assert_eq!(decoded.capture.state, StreamCaptureState::Stopped);
}

#[test]
fn stream_runtime_capabilities_match_enabled_registry() {
    fn kind_label(kind: CodecKind) -> &'static str {
        match kind {
            CodecKind::Decoder => "decoder",
            CodecKind::Encoder => "encoder",
        }
    }

    let runtime = stream_runtime_capabilities().expect("runtime capabilities");
    let registry = CodecRegistry::list_enabled_codecs().expect("enabled codecs");

    let actual: BTreeSet<_> = registry
        .into_iter()
        .flat_map(|(fourcc, codecs)| {
            codecs
                .into_iter()
                .map(move |desc| (kind_label(desc.kind).to_string(), fourcc.to_string(), desc.name.to_string(), desc.impl_name.to_string(), desc.input.to_string(), desc.output.to_string()))
        })
        .collect();

    let advertised: BTreeSet<_> = runtime
        .codecs
        .iter()
        .map(|codec| (kind_label(codec.kind).to_string(), codec.fourcc.clone(), codec.name.clone(), codec.implementation.clone(), codec.input.clone(), codec.output.clone()))
        .collect();

    assert_eq!(advertised, actual);
    assert_eq!(runtime.default_encoder_id, default_stream_encoder_selector());
    assert_eq!(runtime.default_decoder_ids_by_capture_format, default_decoder_ids_by_capture_format());
}

#[test]
fn cached_stream_runtime_capabilities_exposes_cache_metrics() {
    reset_stream_runtime_capabilities_cache_for_tests();

    let initial = stream_runtime_capabilities_cache_snapshot();
    assert_eq!(initial.entries, 0);
    assert_eq!(initial.hits, 0);
    assert_eq!(initial.misses, 0);
    assert_eq!(initial.refreshes, 0);

    let first = cached_stream_runtime_capabilities().expect("runtime capabilities");
    let after_first = stream_runtime_capabilities_cache_snapshot();
    assert_eq!(after_first.entries, 1);
    assert_eq!(after_first.hits, 0);
    assert_eq!(after_first.misses, 1);
    assert_eq!(after_first.refreshes, 1);
    assert!(after_first.last_error.is_none());

    let second = cached_stream_runtime_capabilities().expect("runtime capabilities");
    let after_second = stream_runtime_capabilities_cache_snapshot();
    assert_eq!(after_second.entries, 1);
    assert_eq!(after_second.hits, 1);
    assert_eq!(after_second.misses, 1);
    assert_eq!(after_second.refreshes, 1);
    assert_eq!(first.codecs, second.codecs);
}

#[test]
fn encoder_settings_kind_for_selector_supports_generated_runtime_ids() {
    assert!(matches!(empty_encoder_settings_for_selector(Some("h264_v4l2m2m")), Some(EncoderSettings::H264 { .. })));
    assert!(matches!(empty_encoder_settings_for_selector(Some("hevc_v4l2m2m")), Some(EncoderSettings::H265 { .. })));
    assert!(matches!(empty_encoder_settings_for_selector(Some("turbojpeg")), Some(EncoderSettings::Turbojpeg { .. })));
}

#[test]
fn codec_family_spec_matches_styx_runtime_specs() {
    #[derive(Debug, Deserialize)]
    struct CodecFamiliesSpec {
        encoder_families: Vec<CodecFamilySpec>,
    }

    #[derive(Debug, Deserialize)]
    struct CodecFamilySpec {
        id: String,
        selector_id: String,
        selector_aliases: Vec<String>,
        runtime_implementation_aliases: Vec<String>,
        runtime_name_aliases: Vec<String>,
        output_fourcc_aliases: Vec<String>,
        recording_codec: Option<String>,
    }

    let spec: CodecFamiliesSpec = toml::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tools/api-codegen/codec-families.toml"))).expect("parse codec family spec");

    assert_eq!(spec.encoder_families.len(), styx::runtime_codec::ENCODER_FAMILY_SPECS.len());

    for family in spec.encoder_families {
        let runtime = styx::runtime_codec::ENCODER_FAMILY_SPECS.iter().find(|spec| spec.id == family.id).expect("matching styx codec family");

        assert_eq!(runtime.selector_id, family.selector_id);
        assert_eq!(runtime.selector_aliases, family.selector_aliases);
        assert_eq!(runtime.runtime_implementation_aliases, family.runtime_implementation_aliases);
        assert_eq!(runtime.runtime_name_aliases, family.runtime_name_aliases);
        assert_eq!(runtime.output_fourcc_aliases, family.output_fourcc_aliases);
        assert_eq!(runtime.recording_codec, family.recording_codec.as_deref());
    }
}

#[test]
fn preview_format_for_encoder_selector_uses_styx_codec_families() {
    assert_eq!(preview_format_for_encoder_selector(Some("turbojpeg")), "mjpeg");
    assert_eq!(preview_format_for_encoder_selector(Some("mozjpeg")), "mjpeg");
    assert_eq!(preview_format_for_encoder_selector(Some("mjpeg")), "mjpeg");
    assert_eq!(preview_format_for_encoder_selector(Some("h264_v4l2m2m")), "h264");
    assert_eq!(preview_format_for_encoder_selector(Some("hevc_v4l2m2m")), "h265");
    assert_eq!(preview_format_for_encoder_selector(None), "unknown");
}

#[test]
fn normalize_requested_stream_encoder_preserves_runtime_implementation_ids() {
    let mut exact = sample_manifest();
    exact.encoder = RequestedEncoderConfig::enabled(Some("h264_v4l2m2m".to_string()), None);
    normalize_requested_stream_encoder(&mut exact);
    assert_eq!(exact.encoder.id(), Some("h264_v4l2m2m"));

    let mut avc_alias = sample_manifest();
    avc_alias.encoder = RequestedEncoderConfig::enabled(Some("avc".to_string()), None);
    normalize_requested_stream_encoder(&mut avc_alias);
    assert_eq!(avc_alias.encoder.id(), Some("h264"));

    let mut jpeg_alias = sample_manifest();
    jpeg_alias.encoder = RequestedEncoderConfig::enabled(Some("jpeg".to_string()), None);
    normalize_requested_stream_encoder(&mut jpeg_alias);
    assert_eq!(jpeg_alias.encoder.id(), Some("mjpeg"));
}

#[test]
fn stream_manifest_rejects_legacy_shadow_recorder_field() {
    let mut payload = sample_manifest_json();
    let object = payload.as_object_mut().expect("manifest object");
    object.insert(
        "encoder".to_string(),
        serde_json::json!({
            "state": "enabled",
            "id": "hevc_v4l2m2m"
        }),
    );
    object.insert("shadow_recorder_enabled".to_string(), serde_json::json!(true));

    let err = serde_json::from_value::<StreamManifest>(payload).expect_err("legacy shadow recorder field should fail");
    assert!(err.to_string().contains("unknown field `shadow_recorder_enabled`"));
}

#[test]
fn encoder_settings_accepts_rational_framerate_json() {
    let payload = serde_json::json!({
        "kind": "h264",
        "framerate": { "numerator": 30, "denominator": 1 }
    });
    let parsed: EncoderSettings = serde_json::from_value(payload).expect("decode encoder settings");
    let rate = parsed.framerate().expect("framerate");
    assert_eq!(rate.numerator, 30);
    assert_eq!(rate.denominator, 1);
}

#[test]
fn encoder_settings_rejects_legacy_fps_framerate_json() {
    let payload = serde_json::json!({
        "kind": "h264",
        "framerate": { "fps": 29.97 }
    });
    let err = serde_json::from_value::<EncoderSettings>(payload).expect_err("legacy fps form should fail");
    let message = err.to_string();
    assert!(message.contains("fps") || message.contains("numerator") || message.contains("denominator"));
}

#[test]
fn stream_manifest_defaults_recording_mode_to_disabled_when_omitted() {
    let payload = sample_manifest_json();
    let parsed: StreamManifest = serde_json::from_value(payload).expect("decode manifest");
    assert_eq!(parsed.schema_version, CURRENT_STREAM_CONFIG_SCHEMA_VERSION);
    assert_eq!(parsed.recording_mode, StreamRecordingMode::Disabled);
    assert_eq!(parsed.host_buffer, super::default_host_buffer());
    assert!(!parsed.pipeline_enabled);
    assert_eq!(parsed.preview_jpeg_quality, 65);
}

fn sample_manifest_json() -> serde_json::Value {
    serde_json::json!({
        "schema_version": CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
        "identity": {},
        "capture": {
            "device_keys": [],
            "backend": "Virtual",
            "handle": { "type": "virtual" },
            "mode": {
                "format": {
                    "code": "RGB3",
                    "resolution": { "width": 1, "height": 1 },
                    "color": "Srgb"
                },
                "interval": null
            },
            "controls": []
        }
    })
}

#[test]
fn stream_manifest_serializes_current_schema_version() {
    let manifest = sample_manifest();
    let payload = serde_json::to_value(&manifest).expect("encode manifest");
    assert_eq!(payload.get("schema_version").and_then(serde_json::Value::as_u64), Some(CURRENT_STREAM_CONFIG_SCHEMA_VERSION as u64));
}

#[test]
fn stream_manifest_rejects_versionless_json() {
    let mut payload = sample_manifest_json();
    payload.as_object_mut().expect("manifest object").remove("schema_version");

    let err = serde_json::from_value::<StreamManifest>(payload).expect_err("versionless manifest should fail");
    assert!(err.to_string().contains("schema_version is required"));
}

#[test]
fn stream_manifest_rejects_unknown_future_schema_version() {
    let mut payload = sample_manifest_json();
    payload.as_object_mut().expect("manifest object").insert("schema_version".to_string(), serde_json::json!(CURRENT_STREAM_CONFIG_SCHEMA_VERSION + 1));

    let err = serde_json::from_value::<StreamManifest>(payload).expect_err("future schema version should fail");
    assert!(err.to_string().contains("unsupported stream manifest schema_version"));
}

#[test]
fn stream_manifest_accepts_typed_encoder_json() {
    let mut payload = sample_manifest_json();
    payload.as_object_mut().expect("manifest object").insert(
        "encoder".to_string(),
        serde_json::json!({
            "state": "enabled",
            "id": "h264",
            "settings": {
                "kind": "h264",
                "bitrate": 4_000_000,
                "thread_count": 2
            }
        }),
    );
    let parsed: StreamManifest = serde_json::from_value(payload).expect("decode manifest");
    match parsed.encoder {
        RequestedEncoderConfig::Enabled { id, settings } => {
            assert_eq!(id.as_deref(), Some("h264"));
            let settings = settings.expect("encoder settings");
            assert_eq!(settings.bitrate(), Some(4_000_000));
            assert_eq!(settings.thread_count(), Some(2));
        }
        RequestedEncoderConfig::Disabled => panic!("typed encoder config should remain enabled"),
    }
}

#[test]
fn stream_manifest_accepts_typed_decoder_json() {
    let mut payload = sample_manifest_json();
    payload.as_object_mut().expect("manifest object").insert(
        "decoder".to_string(),
        serde_json::json!({
            "state": "enabled",
            "id": "nv12-luma",
            "settings": {
                "fps_limit": 24.0,
                "rotation_degrees": 90,
                "mirror_horizontal": true
            }
        }),
    );
    let parsed: StreamManifest = serde_json::from_value(payload).expect("decode manifest");
    match parsed.decoder {
        RequestedDecoderConfig::Enabled { id, settings } => {
            assert_eq!(id.as_deref(), Some("nv12-luma"));
            let settings = settings.expect("decoder settings");
            assert_eq!(settings.fps_limit, Some(24.0));
            assert_eq!(settings.rotation_degrees, Some(90));
            assert_eq!(settings.mirror_horizontal, Some(true));
        }
        RequestedDecoderConfig::Disabled => panic!("typed decoder config should remain enabled"),
    }
}

#[test]
fn stream_manifest_rejects_mixed_new_and_legacy_encoder_json() {
    let mut payload = sample_manifest_json();
    let object = payload.as_object_mut().expect("manifest object");
    object.insert("encoder".to_string(), serde_json::json!({ "state": "enabled", "id": "h264" }));
    object.insert("encoder_id".to_string(), serde_json::json!("turbojpeg"));

    let err = serde_json::from_value::<StreamManifest>(payload).expect_err("mixed encoder config should fail");
    assert!(err.to_string().contains("unknown field `encoder_id`"));
}

#[test]
fn stream_manifest_rejects_mixed_new_and_legacy_decoder_json() {
    let mut payload = sample_manifest_json();
    let object = payload.as_object_mut().expect("manifest object");
    object.insert("decoder".to_string(), serde_json::json!({ "state": "enabled", "id": "nv12-luma" }));
    object.insert("decoder_id".to_string(), serde_json::json!("h264"));

    let err = serde_json::from_value::<StreamManifest>(payload).expect_err("mixed decoder config should fail");
    assert!(err.to_string().contains("unknown field `decoder_id`"));
}

#[test]
fn stream_manifest_rejects_legacy_disabled_encoder_with_settings() {
    let mut payload = sample_manifest_json();
    let object = payload.as_object_mut().expect("manifest object");
    object.insert("encoder_enabled".to_string(), serde_json::json!(false));
    object.insert(
        "encoder_settings".to_string(),
        serde_json::json!({
            "bitrate": 2_000_000
        }),
    );

    let err = serde_json::from_value::<StreamManifest>(payload).expect_err("legacy disabled encoder should not accept settings");
    assert!(err.to_string().contains("unknown field `encoder_enabled`"));
}

#[test]
fn stream_manifest_rejects_legacy_disabled_decoder_with_settings() {
    let mut payload = sample_manifest_json();
    let object = payload.as_object_mut().expect("manifest object");
    object.insert("decoder_enabled".to_string(), serde_json::json!(false));
    object.insert(
        "decoder_settings".to_string(),
        serde_json::json!({
            "rotation_degrees": 90
        }),
    );

    let err = serde_json::from_value::<StreamManifest>(payload).expect_err("legacy disabled decoder should not accept settings");
    assert!(err.to_string().contains("unknown field `decoder_enabled`"));
}

#[test]
fn stream_manifest_rejects_legacy_boolean_recording_mode() {
    let mut payload = sample_manifest_json();
    payload.as_object_mut().expect("manifest object").insert("recording_mode".to_string(), serde_json::json!(true));

    let err = serde_json::from_value::<StreamManifest>(payload).expect_err("legacy boolean recording mode should fail");
    assert!(err.to_string().contains("invalid type"));
}

#[test]
fn stream_manifest_derives_pipeline_enabled_from_pipeline_state_when_omitted() {
    let mut payload = sample_manifest_json();
    let object = payload.as_object_mut().expect("manifest object");
    object.insert("pipelines".to_string(), serde_json::json!([{ "pipeline_id": uuid::Uuid::new_v4(), "pipeline_graph": null, "pipeline_output": "overlay" }]));
    object.insert("active_pipeline_id".to_string(), serde_json::json!(uuid::Uuid::new_v4()));

    let parsed: StreamManifest = serde_json::from_value(payload).expect("decode manifest");
    assert!(parsed.pipeline_enabled);
}

#[test]
fn stream_manifest_defaults_preview_quality_when_encoder_disabled() {
    let mut payload = sample_manifest_json();
    payload.as_object_mut().expect("manifest object").insert("encoder".to_string(), serde_json::json!({ "state": "disabled" }));

    let parsed: StreamManifest = serde_json::from_value(payload).expect("decode manifest");
    assert_eq!(parsed.preview_jpeg_quality, 30);
}
