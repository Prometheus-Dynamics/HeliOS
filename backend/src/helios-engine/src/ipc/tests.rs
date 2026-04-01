use super::*;
use lib_ipc::types::CommandId;
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

#[test]
fn start_command_round_trips_over_bincode() {
    let config = bincode::config::standard();
    let base_manifest = sample_manifest();

    // Prove basic primitives round-trip.
    let encoded_identity = bincode::serde::encode_to_vec(&base_manifest.identity, config).expect("encode identity");
    let decoded_identity: crate::identity::DeviceIdentity = bincode::serde::decode_from_slice(&encoded_identity, config).expect("decode identity").0;
    assert_eq!(decoded_identity.alias, base_manifest.identity.alias);

    let capture = &base_manifest.capture;
    let encoded_handle = bincode::serde::encode_to_vec(&capture.handle, config).expect("encode handle");
    let decoded_handle: crate::capture::BackendHandle = bincode::serde::decode_from_slice(&encoded_handle, config).expect("decode handle").0;
    assert!(matches!(decoded_handle, crate::capture::BackendHandle::Virtual));
    let encoded_backend = bincode::serde::encode_to_vec(capture.backend, config).expect("encode backend");
    let decoded_backend: crate::capture::BackendKind = bincode::serde::decode_from_slice(&encoded_backend, config).expect("decode backend").0;
    assert_eq!(decoded_backend, crate::capture::BackendKind::Virtual);
    let encoded_mode = bincode::serde::encode_to_vec(capture.mode.clone(), config).expect("encode mode");
    let decoded_mode: crate::capture::ModeId = bincode::serde::decode_from_slice(&encoded_mode, config).expect("decode mode").0;
    assert_eq!(decoded_mode, capture.mode);

    let encoded_capture = bincode::serde::encode_to_vec(capture, config).expect("encode capture");
    let decoded_capture: crate::capture::CaptureConfig = bincode::serde::decode_from_slice(&encoded_capture, config).expect("decode capture").0;
    assert_eq!(decoded_capture.backend, capture.backend);

    let encoded_manifest = bincode::serde::encode_to_vec(&base_manifest, config).expect("encode manifest");
    let decoded_manifest: StreamManifest = bincode::serde::decode_from_slice(&encoded_manifest, config).expect("decode manifest").0;
    assert_eq!(decoded_manifest.capture.backend, base_manifest.capture.backend);

    let command = EngineCommand::Start { command_id: CommandId::new(), manifest: Box::new(base_manifest.clone().resolve()) };
    let encoded = bincode::encode_to_vec(command, config).expect("encode command");
    let (decoded, _): (EngineCommand, usize) = bincode::decode_from_slice(&encoded, config).expect("decode command");
    if let EngineCommand::Start { manifest: round_trip, .. } = decoded {
        assert_eq!(round_trip.capture.backend, base_manifest.capture.backend);
    } else {
        panic!("unexpected command after decode");
    }
}

#[test]
fn stream_list_event_round_trips() {
    let (_, descriptor) = crate::capture::default_virtual_device().backends.into_iter().next().map(|b| b.descriptor).map(|d| ((), d)).unwrap();
    let summary = StreamSummary { stream_id: uuid::Uuid::new_v4(), descriptor, manifest: sample_manifest().resolve(), status: StreamStatus::default(), runtime: StreamRuntimeState::default() };
    let event = EngineEvent::StreamList { command_id: CommandId::new(), streams: vec![summary.clone()] };
    let config = bincode::config::standard();
    let encoded = bincode::encode_to_vec(event, config).expect("encode event");
    let (decoded, _): (EngineEvent, usize) = bincode::decode_from_slice(&encoded, config).expect("decode event");
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
fn runtime_state_round_trips_over_bincode() {
    let config = bincode::config::standard();
    let runtime = StreamRuntimeState::default();

    let encoded_capture = bincode::serde::encode_to_vec(&runtime.capture, config).expect("encode capture runtime");
    let _: StreamCaptureRuntimeState = bincode::serde::decode_from_slice(&encoded_capture, config).expect("decode capture runtime").0;

    let encoded_codecs = bincode::serde::encode_to_vec(&runtime.codecs, config).expect("encode codec runtime");
    let _: StreamCodecChainRuntimeState = bincode::serde::decode_from_slice(&encoded_codecs, config).expect("decode codec runtime").0;

    let encoded_demand = bincode::serde::encode_to_vec(&runtime.demand, config).expect("encode demand runtime");
    let _: StreamDemandRuntimeState = bincode::serde::decode_from_slice(&encoded_demand, config).expect("decode demand runtime").0;

    let encoded_recording = bincode::serde::encode_to_vec(&runtime.recording, config).expect("encode recording runtime");
    let _: StreamRecordingRuntimeState = bincode::serde::decode_from_slice(&encoded_recording, config).expect("decode recording runtime").0;

    let encoded_pipeline = bincode::serde::encode_to_vec(&runtime.pipeline, config).expect("encode pipeline runtime");
    let _: StreamPipelineRuntimeState = bincode::serde::decode_from_slice(&encoded_pipeline, config).expect("decode pipeline runtime").0;

    let encoded = bincode::serde::encode_to_vec(&runtime, config).expect("encode stream runtime");
    let decoded: StreamRuntimeState = bincode::serde::decode_from_slice(&encoded, config).expect("decode stream runtime").0;
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
fn encoder_settings_accepts_legacy_fps_framerate_json() {
    let payload = serde_json::json!({
        "kind": "h264",
        "framerate": { "fps": 29.97 }
    });
    let parsed: EncoderSettings = serde_json::from_value(payload).expect("decode encoder settings");
    let rate = parsed.framerate().expect("framerate");
    assert_eq!(rate.numerator, 2997);
    assert_eq!(rate.denominator, 100);
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
fn stream_manifest_accepts_legacy_versionless_json() {
    let parsed: StreamManifest = serde_json::from_value(sample_manifest_json()).expect("decode legacy manifest");
    assert_eq!(parsed.schema_version, CURRENT_STREAM_CONFIG_SCHEMA_VERSION);
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
    assert!(err.to_string().contains("may not mix `encoder`"));
}

#[test]
fn stream_manifest_rejects_mixed_new_and_legacy_decoder_json() {
    let mut payload = sample_manifest_json();
    let object = payload.as_object_mut().expect("manifest object");
    object.insert("decoder".to_string(), serde_json::json!({ "state": "enabled", "id": "nv12-luma" }));
    object.insert("decoder_id".to_string(), serde_json::json!("h264"));

    let err = serde_json::from_value::<StreamManifest>(payload).expect_err("mixed decoder config should fail");
    assert!(err.to_string().contains("may not mix `decoder`"));
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
    assert!(err.to_string().contains("legacy encoder_enabled=false"));
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
    assert!(err.to_string().contains("legacy decoder_enabled=false"));
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
