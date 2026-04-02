use super::*;
use crate::ipc::{RequestedDecoderConfig, RequestedEncoderConfig, StreamManifest, CURRENT_STREAM_CONFIG_SCHEMA_VERSION};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use styx::core::controls::{Access, ControlId as StyxControlId, ControlKind, ControlMeta, ControlMetadata, ControlValue};

fn sample_manifest_for_encoder_defaults(width: u32, height: u32) -> ResolvedStreamConfig {
    let identity = crate::identity::DeviceIdentity { id: None, alias: None, hardware_id: None };
    let format = MediaFormat::new(FourCc::new(*b"RGB3"), Resolution::new(width, height).expect("resolution"), ColorSpace::Srgb);
    let capture = crate::capture::CaptureConfig {
        device_keys: vec![],
        device_identity: None,
        backend: crate::capture::BackendKind::Virtual,
        handle: crate::capture::BackendHandle::Virtual,
        mode: crate::capture::ModeId { format, interval: None },
        target_fps: None,
        interval: None,
        controls: vec![],
        enable_tdn_output: false,
    };
    StreamManifest {
        schema_version: CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
        identity,
        capture,
        host_buffer: crate::ipc::default_host_buffer(),
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
        encoder: RequestedEncoderConfig::enabled(Some("h264".to_string()), None),
        decoder: RequestedDecoderConfig::default(),
        preview_jpeg_quality: 30,
        recording_mode: crate::ipc::default_recording_mode(),
        start_on_boot: false,
    }
    .resolve()
}

fn file_video_descriptor(start_id: u32, stop_id: u32, default_stop: u32) -> CaptureDescriptor {
    CaptureDescriptor {
        modes: Vec::new(),
        controls: vec![
            ControlMeta {
                id: StyxControlId(start_id),
                name: "file.video.sample.start_frame".to_string(),
                kind: ControlKind::Uint,
                access: Access::ReadWrite,
                min: ControlValue::Uint(0),
                max: ControlValue::Uint(u32::MAX),
                default: ControlValue::Uint(0),
                step: Some(ControlValue::Uint(1)),
                menu: None,
                metadata: ControlMetadata::default(),
            },
            ControlMeta {
                id: StyxControlId(stop_id),
                name: "file.video.sample.stop_frame".to_string(),
                kind: ControlKind::Uint,
                access: Access::ReadWrite,
                min: ControlValue::Uint(0),
                max: ControlValue::Uint(u32::MAX),
                default: ControlValue::Uint(default_stop),
                step: Some(ControlValue::Uint(1)),
                menu: None,
                metadata: ControlMetadata::default(),
            },
        ],
    }
}

#[test]
fn calibration_mode_output_port_uses_frame_for_calibration() {
    assert_eq!(calibration_mode_output_port(CALIBRATION_TEMPLATE_ID), "frame");
    assert_eq!(calibration_mode_output_port(UNDISTORT_TEMPLATE_ID), "frame");
}

#[test]
fn normalize_output_for_raw_pipeline_rejects_invalid_ports() {
    assert_eq!(normalize_output_for_pipeline(Some("raw".to_string()), Some(RAW_STREAM_PIPELINE_UUID)).expect("raw should be accepted").as_deref(), Some("raw"));
    assert_eq!(normalize_output_for_pipeline(Some("undistorted".to_string()), Some(RAW_STREAM_PIPELINE_UUID)).expect("undistorted should be accepted").as_deref(), Some("undistorted"));
    assert!(normalize_output_for_pipeline(Some("frame".to_string()), Some(RAW_STREAM_PIPELINE_UUID)).is_err());
    assert!(normalize_output_for_pipeline(Some("overlay".to_string()), Some(RAW_STREAM_PIPELINE_UUID)).is_err());
}

#[test]
fn calibration_mode_host_buffer_forces_low_latency() {
    assert_eq!(calibration_mode_host_buffer(8), CALIBRATION_MODE_HOST_BUFFER);
    assert_eq!(calibration_mode_host_buffer(0), CALIBRATION_MODE_HOST_BUFFER);
}

#[test]
fn normalize_calibration_dictionary_name_accepts_dict_aliases() {
    assert_eq!(normalize_calibration_dictionary_name("4x4_1000").as_deref(), Some("4x4_1000"));
    assert_eq!(normalize_calibration_dictionary_name("dict4x4_1000").as_deref(), Some("4x4_1000"));
    assert_eq!(normalize_calibration_dictionary_name("DICT_4X4_1000").as_deref(), Some("4x4_1000"));
    assert_eq!(normalize_calibration_dictionary_name("apriltag_36h11").as_deref(), Some("apriltag_36h11"));
    assert_eq!(normalize_calibration_dictionary_name("bogus"), None);
}

#[test]
fn calibration_dictionary_max_id_parses_aruco_sizes() {
    assert_eq!(calibration_dictionary_max_id("4x4_50"), Some(49));
    assert_eq!(calibration_dictionary_max_id("4x4_1000"), Some(999));
    assert_eq!(calibration_dictionary_max_id("apriltag_36h11"), None);
    assert_eq!(calibration_dictionary_max_id(""), None);
}

#[test]
fn default_encoder_settings_use_480p_for_1080p_capture() {
    let manifest = sample_manifest_for_encoder_defaults(1920, 1080);
    let output = manifest.encoder.settings.as_ref().and_then(|settings| settings.output_resolution()).expect("output resolution");
    assert_eq!(output.width, 854);
    assert_eq!(output.height, 480);
}

#[test]
fn default_encoder_settings_preserve_aspect_for_16_by_10_capture() {
    let manifest = sample_manifest_for_encoder_defaults(1280, 800);
    let output = manifest.encoder.settings.as_ref().and_then(|settings| settings.output_resolution()).expect("output resolution");
    assert_eq!(output.width, 768);
    assert_eq!(output.height, 480);
}

#[test]
fn default_encoder_settings_set_framerate_and_preview_quality() {
    let manifest = sample_manifest_for_encoder_defaults(1920, 1080);
    let settings = manifest.encoder.settings.expect("encoder settings");
    let framerate = settings.framerate().expect("framerate");
    assert_eq!(framerate.numerator, 60);
    assert_eq!(framerate.denominator, 1);
    assert_eq!(manifest.preview_jpeg_quality, 30);
}

#[test]
fn default_encoder_settings_do_not_override_explicit_values() {
    let manifest = StreamManifest {
        encoder: RequestedEncoderConfig::enabled(
            None,
            Some(crate::ipc::EncoderSettings::H264 {
                bitrate: None,
                gop: None,
                framerate: Some(crate::ipc::FrameRate { numerator: 24, denominator: 1 }),
                thread_count: None,
                output_resolution: Some(crate::ipc::ResolutionHint { width: 1280, height: 720 }),
            }),
        ),
        preview_jpeg_quality: 80,
        ..sample_manifest_for_encoder_defaults(1920, 1080).to_requested_manifest()
    }
    .resolve();
    let settings = manifest.encoder.settings.expect("encoder settings");
    let output = settings.output_resolution().expect("output resolution");
    assert_eq!(output.width, 1280);
    assert_eq!(output.height, 720);
    let framerate = settings.framerate().expect("framerate");
    assert_eq!(framerate.numerator, 24);
    assert_eq!(framerate.denominator, 1);
    assert_eq!(manifest.preview_jpeg_quality, 80);
}

#[test]
fn patch_dictionary_const_updates_aruco_dictionary_inputs() {
    let mut graph = json!({
        "nodes": [
            { "id": "cv:aruco:decode_quads_hamming", "inputs": ["frame", "quads", "dictionary"] },
            { "id": "cv:aruco:overlay_detections", "inputs": ["frame", "detections"] },
            { "id": "cv:other:example", "inputs": ["dictionary"] }
        ]
    });
    patch_dictionary_const(&mut graph, "4x4_1000");

    let nodes = graph.get("nodes").and_then(|v| v.as_array()).expect("nodes");
    let decode_consts = nodes[0].get("const_inputs").and_then(|v| v.as_array()).expect("decode consts");
    assert!(decode_consts.iter().any(|entry| {
        let pair = entry.as_array().expect("const pair");
        pair[0].as_str() == Some("dictionary") && pair[1].get("value").and_then(|v| v.as_str()) == Some("4x4_1000")
    }));
    assert!(nodes[1].get("const_inputs").is_none());
    assert!(nodes[2].get("const_inputs").is_none());
}

#[test]
fn ensure_calibration_mode_frame_output_sets_host_input_port_metadata() {
    let mut graph = json!({
        "nodes": [
            {
                "id": "io.host_bridge",
                "outputs": ["frame", "roi_x", "roi_y"],
                "metadata": { "host_bridge": { "type": "Bool", "value": true } }
            },
            {
                "id": "io.host_output",
                "inputs": ["detections"],
                "outputs": []
            }
        ],
        "edges": []
    });

    ensure_calibration_mode_frame_output(&mut graph);

    let nodes = graph.get("nodes").and_then(|value| value.as_array()).expect("nodes");
    let bridge_metadata = nodes[0].get("metadata").and_then(|value| value.as_object()).expect("bridge metadata");
    assert_eq!(bridge_metadata.get("helios.host_input_port").and_then(|value| value.get("value")).and_then(|value| value.as_str()), Some("frame"));

    let host_output_inputs = nodes[1].get("inputs").and_then(|value| value.as_array()).expect("host output inputs");
    assert!(host_output_inputs.iter().any(|value| value.as_str() == Some("frame")));
}

#[test]
fn sanitize_file_video_controls_expands_invalid_stop_to_non_empty_range() {
    let descriptor = file_video_descriptor(10, 11, 800);
    let mut controls = vec![ControlAssignment { id: 10, value: CaptureControlValue::Uint(200) }, ControlAssignment { id: 11, value: CaptureControlValue::Uint(120) }];

    sanitize_file_video_frame_controls(&descriptor, &mut controls);

    let start = controls.iter().find(|ctl| ctl.id == 10).and_then(|ctl| control_value_to_u32(&ctl.value));
    let stop = controls.iter().find(|ctl| ctl.id == 11).and_then(|ctl| control_value_to_u32(&ctl.value));
    assert_eq!(start, Some(200));
    assert_eq!(stop, Some(201));
}

#[test]
fn sanitize_file_video_controls_clamps_to_known_frame_count_with_non_empty_range() {
    let descriptor = file_video_descriptor(10, 11, 500);
    let mut controls = vec![ControlAssignment { id: 10, value: CaptureControlValue::Uint(800) }, ControlAssignment { id: 11, value: CaptureControlValue::Uint(700) }];

    sanitize_file_video_frame_controls(&descriptor, &mut controls);

    let start = controls.iter().find(|ctl| ctl.id == 10).and_then(|ctl| control_value_to_u32(&ctl.value));
    let stop = controls.iter().find(|ctl| ctl.id == 11).and_then(|ctl| control_value_to_u32(&ctl.value));
    assert_eq!(start, Some(499));
    assert_eq!(stop, Some(500));
}

#[tokio::test(flavor = "current_thread")]
async fn abort_unregistered_stream_worker_stops_and_joins_thread() {
    let (command_tx, command_rx) = sync_channel::<StreamCommand>(1);
    let stopped = Arc::new(AtomicBool::new(false));
    let stopped_flag = Arc::clone(&stopped);
    let join = std::thread::spawn(move || {
        match command_rx.recv().expect("stop command") {
            StreamCommand::Stop { respond_to } => {
                let _ = respond_to.send(());
            }
            _ => panic!("expected stop command"),
        }
        stopped_flag.store(true, Ordering::SeqCst);
    });

    abort_unregistered_stream_worker(Uuid::nil(), command_tx, join).await;
    assert!(stopped.load(Ordering::SeqCst));
}
