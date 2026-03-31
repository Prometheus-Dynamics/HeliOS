use super::*;
use lib_ipc::types::CommandId;
use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};

fn sample_manifest() -> StreamManifest {
    let identity = crate::identity::DeviceIdentity { id: None, alias: None, hardware_id: None };
    let fmt = MediaFormat::new(FourCc::new(*b"RGB3"), Resolution::new(1, 1).unwrap(), ColorSpace::Srgb);
    let capture = crate::capture::CaptureConfig {
        device_keys: vec![],
        backend: crate::capture::BackendKind::Virtual,
        handle: crate::capture::BackendHandle::Virtual,
        mode: crate::capture::ModeId { format: fmt, interval: None },
        target_fps: None,
        interval: None,
        controls: vec![],
        enable_tdn_output: false,
    };
    StreamManifest {
        identity,
        capture,
        host_buffer: super::default_host_buffer(),
        internal: false,
        pipeline_enabled: None,
        pipelines: Vec::new(),
        active_pipeline_id: None,
        active_pipeline_output: None,
        pipeline_layout: None,
        pipeline_wires: Vec::new(),
        pipeline_host_inputs: std::collections::BTreeMap::new(),
        calibration: None,
        pose: None,
        encoder: RequestedEncoderConfig::default(),
        decoder_enabled: None,
        decoder_id: None,
        decoder_settings: None,
        preview_jpeg_quality: None,
        shadow_recorder_enabled: true,
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
    let summary = StreamSummary { stream_id: uuid::Uuid::new_v4(), descriptor, manifest: sample_manifest().resolve(), status: StreamStatus::default() };
    let event = EngineEvent::StreamList { command_id: CommandId::new(), streams: vec![summary.clone()] };
    let config = bincode::config::standard();
    let encoded = bincode::encode_to_vec(event, config).expect("encode event");
    let (decoded, _): (EngineEvent, usize) = bincode::decode_from_slice(&encoded, config).expect("decode event");
    match decoded {
        EngineEvent::StreamList { streams, .. } => {
            assert_eq!(streams.len(), 1);
            assert_eq!(streams[0].descriptor.modes.len(), summary.descriptor.modes.len());
        }
        other => panic!("unexpected event after decode: {:?}", other),
    }
}

#[test]
fn encoder_settings_accepts_rational_framerate_json() {
    let payload = serde_json::json!({
        "framerate": { "numerator": 30, "denominator": 1 }
    });
    let parsed: EncoderSettings = serde_json::from_value(payload).expect("decode encoder settings");
    let rate = parsed.framerate.expect("framerate");
    assert_eq!(rate.numerator, 30);
    assert_eq!(rate.denominator, 1);
}

#[test]
fn encoder_settings_accepts_legacy_fps_framerate_json() {
    let payload = serde_json::json!({
        "framerate": { "fps": 29.97 }
    });
    let parsed: EncoderSettings = serde_json::from_value(payload).expect("decode encoder settings");
    let rate = parsed.framerate.expect("framerate");
    assert_eq!(rate.numerator, 2997);
    assert_eq!(rate.denominator, 100);
}

#[test]
fn stream_manifest_defaults_shadow_recorder_off_when_omitted() {
    let payload = sample_manifest_json();
    let parsed: StreamManifest = serde_json::from_value(payload).expect("decode manifest");
    assert!(!parsed.shadow_recorder_enabled);
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
fn stream_manifest_accepts_typed_encoder_json() {
    let mut payload = sample_manifest_json();
    payload.as_object_mut().expect("manifest object").insert(
        "encoder".to_string(),
        serde_json::json!({
            "state": "enabled",
            "id": "h264",
            "settings": {
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
            assert_eq!(settings.bitrate, Some(4_000_000));
            assert_eq!(settings.thread_count, Some(2));
        }
        RequestedEncoderConfig::Disabled => panic!("typed encoder config should remain enabled"),
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
