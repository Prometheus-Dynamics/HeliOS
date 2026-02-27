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
        encoder_enabled: None,
        encoder_id: None,
        decoder_enabled: None,
        decoder_id: None,
        encoder_settings: None,
        decoder_settings: None,
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

    let command = EngineCommand::Start { command_id: CommandId::new(), manifest: Box::new(base_manifest.clone()) };
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
    let summary = StreamSummary { stream_id: uuid::Uuid::new_v4(), descriptor, manifest: sample_manifest(), status: StreamStatus::default() };
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
