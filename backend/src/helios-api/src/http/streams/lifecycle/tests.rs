use super::*;
use crate::app_state::ApiAppState;
use crate::ipc::{self, engine::connect_engine_at_for_tests};
use helios_engine::capture::{BackendHandle, BackendKind, CaptureConfig, ModeId};
use helios_engine::identity::DeviceIdentity;
use helios_engine::ipc::{
    EngineCommand, EngineEvent, StreamCaptureRuntimeState, StreamCaptureState, StreamCodecChainRuntimeState, StreamDemandPipelineRuntimeState, StreamDemandRuntimeState,
    StreamGraphDemandRuntimeState, StreamPipelineRuntimeState, StreamRecordingDemandRuntimeState, StreamRecordingRuntimeState, StreamRuntimeCapabilities, StreamRuntimeState, StreamSummary,
    StreamViewerDemandRuntimeState, cached_stream_runtime_capabilities,
};
use helios_engine::stream::StreamFrameDemandMetrics;
use lib_ipc::server;
use lib_ipc::types::{FeatureSet, ProtocolVersion};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

fn sample_ov9782_manifest() -> StreamManifest {
    let format = MediaFormat::new(FourCc::new(*b"NV12"), Resolution::new(1280, 800).unwrap(), ColorSpace::Srgb);
    StreamManifest {
        schema_version: helios_engine::ipc::CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
        identity: DeviceIdentity { id: None, alias: Some("ov9782 cam".to_string()), hardware_id: None },
        capture: CaptureConfig {
            device_keys: vec!["ov9782".to_string()],
            device_identity: None,
            backend: BackendKind::Libcamera,
            handle: BackendHandle::Libcamera { id: "ov9782-main".to_string() },
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
        recording_mode: helios_engine::ipc::default_recording_mode(),
        start_on_boot: false,
    }
}

fn sample_update_test_manifest(stream_id: Uuid, alias: &str) -> StreamManifest {
    let mut manifest = sample_ov9782_manifest();
    manifest.identity.id = Some(stream_id);
    manifest.identity.alias = Some(alias.to_string());
    manifest.identity.hardware_id = Some(format!("{alias}-hw"));
    manifest
}

fn runtime_summary_for_manifest(manifest: &StreamManifest) -> StreamSummary {
    let descriptor = descriptor_from_persisted_manifest(manifest);
    let resolved = manifest.resolve();
    let runtime = StreamRuntimeState {
        capture: StreamCaptureRuntimeState { state: StreamCaptureState::Running, started_at_ms: Some(1), capture_fourcc: Some("NV12".to_string()), ..Default::default() },
        ..Default::default()
    };
    StreamSummary { stream_id: manifest.identity.id.expect("stream id"), descriptor, manifest: resolved, status: runtime.status(), runtime }
}

#[derive(Debug)]
struct MockEngineError(String);

impl std::fmt::Display for MockEngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MockEngineError {}

impl server::RetryableError for MockEngineError {}

#[derive(Debug)]
struct MockEngineState {
    active: Option<StreamSummary>,
    failing_alias: String,
    failed_update_once: bool,
}

struct MockEngineRuntime {
    _tempdir: TempDir,
    socket_path: PathBuf,
    journal_path: PathBuf,
    shutdown: CancellationToken,
    command_task: JoinHandle<()>,
}

impl Drop for MockEngineRuntime {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.command_task.abort();
    }
}

async fn spawn_mock_engine_runtime(initial_summary: StreamSummary, failing_alias: String) -> MockEngineRuntime {
    let tempdir = tempfile::Builder::new().prefix("item26-engine-").tempdir_in(std::env::temp_dir()).expect("tempdir");
    let socket_path = tempdir.path().join("engine.sock");
    let journal_path = tempdir.path().join("engine.journal");
    let listener = UnixListener::bind(&socket_path).expect("bind mock engine");
    let shutdown = CancellationToken::new();
    let runtime: StreamRuntimeCapabilities = cached_stream_runtime_capabilities().expect("runtime capabilities");
    let state = Arc::new(Mutex::new(MockEngineState { active: Some(initial_summary), failing_alias, failed_update_once: false }));
    let (events, _) = broadcast::channel(32);
    let server_config = server::ServerConfig::new(ProtocolVersion::default(), "mock-engine", env!("CARGO_PKG_VERSION").to_string(), FeatureSet::default(), lib_ipc::wire::ServiceKind::Engine)
        .with_snapshot_required(false);
    let shutdown_token = shutdown.clone();
    let command_task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept mock engine");
        let events_tx = events.clone();
        let subscribe = {
            let events = events.clone();
            move || events.subscribe()
        };
        let snapshot = || async { Ok::<_, MockEngineError>(None) };
        let handle_command = move |command: EngineCommand| {
            let state = state.clone();
            let events = events_tx.clone();
            let runtime = runtime.clone();
            async move {
                let event = match command {
                    EngineCommand::List { command_id } => {
                        let active = state.lock().await.active.clone();
                        EngineEvent::StreamList { command_id, streams: active.into_iter().collect() }
                    }
                    EngineCommand::GetStreamRuntimeCapabilities { command_id } => EngineEvent::StreamRuntimeCapabilities { command_id, capabilities: runtime.clone() },
                    EngineCommand::Stop { command_id, stream_id } => {
                        let mut guard = state.lock().await;
                        if guard.active.as_ref().is_some_and(|summary| summary.stream_id == stream_id) {
                            guard.active = None;
                        }
                        EngineEvent::Stopped { command_id, stream_id }
                    }
                    EngineCommand::Start { command_id, manifest } => {
                        let requested = manifest.to_requested_manifest();
                        let mut guard = state.lock().await;
                        if !guard.failed_update_once && requested.identity.alias.as_deref() == Some(guard.failing_alias.as_str()) {
                            guard.failed_update_once = true;
                            EngineEvent::Nack { command_id, code: EngineErrorCode::InvalidInput, reason: "synthetic update failure".to_string(), retryable: false }
                        } else {
                            let summary = runtime_summary_for_manifest(&requested);
                            let descriptor = summary.descriptor.clone();
                            guard.active = Some(summary);
                            EngineEvent::Started { command_id, stream_id: requested.identity.id.expect("stream id"), descriptor }
                        }
                    }
                    other => return Err(MockEngineError(format!("unexpected engine command in lifecycle rollback test: {other:?}"))),
                };
                let _ = events.send(event.clone());
                Ok::<_, MockEngineError>(Some(event))
            }
        };

        let result = server::run_snapshot_server(stream, shutdown_token, server_config, subscribe, snapshot, handle_command, server::no_heartbeat(), server::log_accept).await;
        if let Err(err) = result {
            panic!("mock engine server failed: {err}");
        }
    });

    MockEngineRuntime { _tempdir: tempdir, socket_path, journal_path, shutdown, command_task }
}

async fn connect_test_state(mock: &MockEngineRuntime) -> AppState {
    let engine = connect_engine_at_for_tests(mock.socket_path.clone(), mock.journal_path.clone()).await.expect("connect mock engine");
    Arc::new(ApiAppState::new(Arc::new(ipc::IpcHandles::for_tests(engine))))
}

#[test]
fn ov9782_defaults_disable_tdn_output_when_noise_reduction_is_missing() {
    let mut manifest = sample_ov9782_manifest();

    apply_new_ov9782_defaults(&mut manifest);

    assert_eq!(manifest.capture.target_fps, Some(OV9782_DEFAULT_LIBCAMERA_TARGET_FPS));
    assert!(!manifest.capture.enable_tdn_output);
    assert_eq!(
        manifest.capture.controls.iter().find(|ctl| ctl.id == LIBCAMERA_NOISE_REDUCTION_MODE).map(|ctl| ctl.value.clone()),
        Some(helios_engine::capture::CaptureControlValue::Int(OV9782_NOISE_REDUCTION_OFF))
    );
}

#[test]
fn ov9782_defaults_keep_tdn_output_disabled_even_with_nonzero_noise_reduction() {
    let mut manifest = sample_ov9782_manifest();
    manifest.capture.controls.push(helios_engine::capture::ControlAssignment { id: LIBCAMERA_NOISE_REDUCTION_MODE, value: helios_engine::capture::CaptureControlValue::Int(1) });

    apply_new_ov9782_defaults(&mut manifest);

    assert!(!manifest.capture.enable_tdn_output);
}

#[test]
fn ensure_descriptor_has_mode_adds_requested_mode_to_existing_snapshot() {
    let manifest = sample_ov9782_manifest();
    let alternate_format = MediaFormat::new(FourCc::new(*b"RGB3"), Resolution::new(640, 480).unwrap(), ColorSpace::Srgb);
    let mut descriptor = helios_engine::capture::CaptureDescriptor {
        modes: vec![helios_engine::capture::CaptureMode {
            id: ModeId { format: alternate_format, interval: None },
            format: alternate_format,
            intervals: Default::default(),
            interval_stepwise: None,
        }],
        controls: Vec::new(),
    };

    ensure_descriptor_has_mode(&mut descriptor, &manifest);

    assert!(descriptor.modes.iter().any(|mode| mode.id == manifest.capture.mode));
    assert_eq!(descriptor.modes.len(), 2);
}

fn sample_stream_runtime() -> StreamRuntimeState {
    StreamRuntimeState {
        capture: StreamCaptureRuntimeState {
            state: StreamCaptureState::Running,
            started_at_ms: Some(42),
            capture_fourcc: Some("YUYV".to_string()),
            disabled_since_ms: None,
            disabled_reason: None,
        },
        codecs: StreamCodecChainRuntimeState {
            capture_input_fourcc: Some("YUYV".to_string()),
            decoder_impl: Some("yuyv-cpu".to_string()),
            encoder_input_fourcc: Some("RG24".to_string()),
            encoder_impl: Some("turbojpeg".to_string()),
            encoder_output_fourcc: Some("MJPG".to_string()),
        },
        demand: StreamDemandRuntimeState {
            frame: StreamFrameDemandMetrics {
                raw_receiver_count: 1,
                host_receiver_count: 2,
                preview_demand_active: true,
                encode_demand_active: true,
                graph_sample_demand_active: false,
                needs_decoded_image: true,
                graph_has_image_output: true,
                graph_has_executor: false,
            },
            encoder: helios_engine::stream::StreamEncoderDemandMetrics {
                broadcast_receiver_count: 1,
                managed_consumer_count: 1,
                managed_consumer_last_seen_ms: 99,
                encoder_demand_active: true,
                encoder_worker_running: true,
            },
            viewers: StreamViewerDemandRuntimeState { raw_receiver_count: 1, host_receiver_count: 2, preview_viewer_active: true },
            graph: StreamGraphDemandRuntimeState { output_sample_pending: false, has_image_output: true, has_executor: false, image_output_active: true, execution_active: false },
            recording: StreamRecordingDemandRuntimeState { recording_session_active: true, shadow_recorder_active: false },
            pipeline: StreamDemandPipelineRuntimeState {
                decoded_image_active: true,
                encoded_output_active: true,
                preview_transport_active: true,
                graph_image_output_active: true,
                graph_execution_active: false,
                encoded_passthrough_possible: false,
                encoded_passthrough_active: false,
                live_active: true,
            },
            live_active: true,
        },
        recording: StreamRecordingRuntimeState { state: helios_engine::ipc::StreamRecordingState::Active, started_at_ms: Some(77) },
        pipeline: StreamPipelineRuntimeState {
            enabled: true,
            active_pipeline_id: Some(crate::http::streams::RAW_PIPELINE_UUID),
            active_output_key: Some("raw".to_string()),
            pipeline_count: 1,
            disabled: false,
            disabled_since_ms: None,
            disabled_reason: None,
        },
    }
}

#[test]
fn stream_inspect_info_surfaces_runtime_sections() {
    let manifest = sample_ov9782_manifest();
    let descriptor = descriptor_from_persisted_manifest(&manifest);
    let resolved = manifest.resolve();
    let info = build_stream_info(Uuid::nil(), descriptor.clone(), resolved.clone(), None, Some(sample_stream_runtime()));
    let inspect = StreamInspectInfo::from(info);

    assert_eq!(inspect.id, Uuid::nil());
    assert_eq!(inspect.descriptor.modes.len(), descriptor.modes.len());
    assert_eq!(inspect.descriptor.controls.len(), descriptor.controls.len());
    assert_eq!(inspect.descriptor.modes[0].id, descriptor.modes[0].id);
    assert_eq!(inspect.resolved.identity.id, resolved.identity.id);
    assert_eq!(inspect.resolved.capture.mode, resolved.capture.mode);
    assert_eq!(inspect.resolved.encoder.codec_id, resolved.encoder.codec_id);
    assert_eq!(inspect.capture.as_ref().and_then(|capture| capture.capture_fourcc.as_deref()), Some("YUYV"));
    assert_eq!(inspect.codec_chain.as_ref().and_then(|codec| codec.encoder_impl.as_deref()), Some("turbojpeg"));
    assert!(inspect.consumer_demand.as_ref().is_some_and(|demand| demand.live_active));
    assert!(inspect.consumer_demand.as_ref().is_some_and(|demand| demand.recording.recording_session_active));
    assert!(inspect.consumer_demand.as_ref().is_some_and(|demand| demand.pipeline.encoded_output_active));
    assert_eq!(inspect.recording.as_ref().and_then(|recording| recording.started_at_ms), Some(77));
    assert_eq!(inspect.pipeline.as_ref().and_then(|pipeline| pipeline.active_output_key.as_deref()), Some("raw"));
}

#[test]
fn stream_inspect_info_omits_runtime_sections_when_unavailable() {
    let manifest = sample_ov9782_manifest();
    let descriptor = descriptor_from_persisted_manifest(&manifest);
    let inspect = StreamInspectInfo::from(build_stream_info(Uuid::nil(), descriptor, manifest.resolve(), None, None));

    assert!(inspect.capture.is_none());
    assert!(inspect.codec_chain.is_none());
    assert!(inspect.consumer_demand.is_none());
    assert!(inspect.recording.is_none());
    assert!(inspect.pipeline.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn update_stream_rolls_back_previous_runtime_when_restart_fails() {
    let stream_id = Uuid::new_v4();
    let original_manifest = sample_update_test_manifest(stream_id, "item26-before");
    let mock = spawn_mock_engine_runtime(runtime_summary_for_manifest(&original_manifest), "item26-after".to_string()).await;
    let state = connect_test_state(&mock).await;

    let mut edited_manifest = original_manifest.clone();
    edited_manifest.identity.alias = Some("item26-after".to_string());
    edited_manifest.identity.hardware_id = Some("item26-after-hw".to_string());
    edited_manifest.preview_jpeg_quality = 47;

    let response = tokio::time::timeout(Duration::from_secs(15), update_stream(state.clone(), stream_id, edited_manifest)).await.expect("update_stream timed out");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let streams = state.engine.list_streams().await.expect("list streams after rollback");
    assert_eq!(streams.len(), 1);
    let restored = &streams[0];
    assert_eq!(restored.stream_id, stream_id);
    assert_eq!(restored.manifest.to_requested_manifest().identity.alias.as_deref(), Some("item26-before"));
    assert_eq!(restored.manifest.to_requested_manifest().preview_jpeg_quality, original_manifest.preview_jpeg_quality);
}
