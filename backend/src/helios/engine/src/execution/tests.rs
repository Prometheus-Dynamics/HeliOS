use std::{
    num::NonZeroU32,
    str::FromStr,
    sync::mpsc,
    time::{Duration, Instant},
};

use daedalus::{
    PluginRegistry, declare_plugin,
    host_bridge::host_port,
    macros::node,
    runtime::{NodeError, plugins::RegistryPluginExt},
};
use helios_vision::{
    image::GrayImage,
    plugin::VisionPlugin,
    testing::{
        DICT_4X4_50,
        render::{paste_warped, render_marker},
    },
};
use orion::control_plane::{ClusterStateEnvelope, ObservedClusterState, ResourceActionResult, ResourceActionStatus, ResourceRecord, ResourceState, TypedConfigValue};
use styx::{
    core::prelude::{BufferPool, ColorSpace, FourCc, FrameMeta, MediaFormat, Resolution, plane_layout_from_dims},
    imports::framelease::FrameLease,
};

use super::{
    bindings::FrameSourceSpec,
    frame_source::{FrameReceive, FrameSource, FrameSourceStatus},
    *,
};
use crate::model::{ExecutionBinding, FrameRequestOptions, GraphRef, PluginRequirement};

/// `graphs/aruco-4x4_50.graph.json` from helios-vision: the document engine workloads run.
const ARUCO_GRAPH_DOCUMENT: &str = include_str!("../../../vision/graphs/aruco-4x4_50.graph.json");

#[node(id = "test.source", outputs("out"))]
fn source() -> Result<i64, NodeError> {
    Ok(7)
}

#[node(id = "test.echo", inputs("inp"), outputs("out"))]
fn echo(inp: String) -> Result<String, NodeError> {
    Ok(inp)
}

#[node(id = "test.frame_passthrough", inputs("frame"), outputs("frame"))]
fn frame_passthrough(frame: FrameLease) -> Result<FrameLease, NodeError> {
    Ok(frame)
}

#[node(id = "test.frame_context", inputs("frame", "context"), outputs("out"))]
fn frame_context(frame: FrameLease, context: String) -> Result<String, NodeError> {
    Ok(serde_json::json!({ "timestamp": frame.meta().timestamp, "context": context }).to_string())
}

declare_plugin!(EngineTestPlugin, "engine.test", [source, echo, frame_passthrough, frame_context]);

/// Frames handed over in-process, standing in for a camera service socket.
struct ChannelFrameSource(mpsc::Receiver<FrameLease>);

impl FrameSource for ChannelFrameSource {
    fn recv(&mut self, wait: Duration) -> FrameReceive {
        match self.0.recv_timeout(wait) {
            Ok(frame) => FrameReceive::Frame(frame),
            Err(mpsc::RecvTimeoutError::Timeout) => FrameReceive::Idle,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(wait);
                FrameReceive::Unavailable("test channel closed".into())
            }
        }
    }

    fn status(&self) -> FrameSourceStatus {
        FrameSourceStatus { connected: true, ..FrameSourceStatus::default() }
    }
}

fn test_registry() -> PluginRegistry {
    let mut plugins = PluginRegistry::new();
    plugins.install_plugin(&EngineTestPlugin::new()).expect("install test plugin");
    plugins.install(&VisionPlugin::new()).expect("install vision plugin");
    plugins
}

fn loaded_plugins() -> Vec<LoadedPlugin> {
    ["engine.test", "helios.vision"].into_iter().map(|name| LoadedPlugin { path: "<test>".into(), plugin_name: Some(name.into()), plugin_version: None, daedalus_version: None }).collect()
}

fn workload(id: &str, graph_json: String, bindings: Vec<ExecutionBinding>) -> ExecutionWorkload {
    ExecutionWorkload {
        workload_id: id.into(),
        artifact_id: format!("artifact.{id}"),
        assigned_node_id: "node-local".into(),
        graph_ref: GraphRef::InlineSpec(graph_json),
        bindings,
        plugin_requirements: Vec::new(),
    }
}

fn binding(input: &str, resource_id: &str) -> ExecutionBinding {
    ExecutionBinding { input: input.into(), resource_id: resource_id.into(), node_id: "node-local".into(), frame_request: FrameRequestOptions::default() }
}

fn state_with(resources: Vec<ResourceRecord>) -> StateSnapshot {
    StateSnapshot {
        state: ClusterStateEnvelope {
            desired: Default::default(),
            observed: ObservedClusterState { resources: resources.into_iter().map(|resource| (resource.resource_id.clone(), resource)).collect(), ..Default::default() },
            applied: Default::default(),
        },
    }
}

fn grey_frame(image: &GrayImage, timestamp: u64) -> FrameLease {
    let (width, height) = (image.width() as u32, image.height() as u32);
    let format = MediaFormat::new(FourCc::from_str("GREY").expect("fourcc"), Resolution::new(width, height).expect("resolution"), ColorSpace::Srgb);
    let layout = plane_layout_from_dims(NonZeroU32::new(width).expect("width"), NonZeroU32::new(height).expect("height"), 1);
    let pool = BufferPool::lazy(layout.len, 1);
    let mut frame = FrameLease::single_plane(FrameMeta::new(format, timestamp), pool.lease(), layout.len, layout.stride);
    frame.copy_slice_to_visible_plane(0, image.data()).expect("copy pixels");
    frame
}

/// A 640x480 scene with ArUco 4x4_50 marker 17.
fn marker_scene() -> GrayImage {
    let mut scene = GrayImage::filled(640, 480, 140);
    let marker = render_marker(&DICT_4X4_50, 17, 16, 1).expect("render marker");
    assert!(paste_warped(&mut scene, &marker, [[200.0, 120.0], [400.0, 140.0], [390.0, 330.0], [190.0, 310.0]]));
    scene
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn frame_spec(input: &str) -> FrameSourceSpec {
    FrameSourceSpec { input: input.into(), resource_id: "camera.test".into(), socket_path: "/nonexistent.sock".into(), request: FrameRequestOptions::default() }
}

/// Spawn a frame driver for `workload` fed through the returned sender.
fn spawn_channel_driver(registry: &PluginRegistry, workload: ExecutionWorkload, input: &str) -> (FrameDrivenExecution, mpsc::Sender<FrameLease>) {
    let graph = graph::compile_workload_graph(registry, &loaded_plugins(), &workload).expect("compile graph");
    let (sender, receiver) = mpsc::channel();
    let driver = FrameDrivenExecution::spawn(workload, graph, vec![frame_spec(input)], vec![Box::new(ChannelFrameSource(receiver))]).expect("spawn driver");
    (driver, sender)
}

#[test]
fn inline_graph_document_produces_output_artifact() {
    let registry = test_registry();
    let src = EngineTestPlugin::new().source.clone().alias("src");
    let graph = registry.graph_builder().expect("graph builder").host_bridge("host").node(&src).connect(&src.outputs.out, &host_port("host", "result")).build();
    let document = registry.graph_document(graph).to_json().expect("document json");
    assert!(document.contains("\"engine.test\""), "requires is filled from the registry: {document}");

    let mut resident = ResidentExecutionSet::default();
    let snapshot = resident.tick_workloads(&ExecutionPlugins { registry: &registry, loaded_plugins: &loaded_plugins() }, &[workload("workload.inline", document, Vec::new())], None, 42);

    assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Running, "{:?}", snapshot.sessions[0].message);
    let output = snapshot.artifacts.iter().find(|artifact| artifact.kind == "host_output:result").expect("result artifact");
    assert_eq!(output.message.as_deref(), Some("{\"value\":7}"));
    assert_eq!(output.artifact_id, "session.workload.inline.result");
}

#[test]
fn bare_graph_json_is_rejected() {
    let registry = test_registry();
    let src = EngineTestPlugin::new().source.clone().alias("src");
    let graph = registry.graph_builder().expect("graph builder").host_bridge("host").node(&src).connect(&src.outputs.out, &host_port("host", "result")).build();
    let bare = serde_json::to_string(&graph).expect("graph json");

    let mut resident = ResidentExecutionSet::default();
    let snapshot = resident.tick_workloads(&ExecutionPlugins { registry: &registry, loaded_plugins: &loaded_plugins() }, &[workload("workload.bare", bare, Vec::new())], None, 42);

    assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Failed);
    assert!(snapshot.sessions[0].message.as_deref().is_some_and(|message| message.contains("GraphDocument")), "{:?}", snapshot.sessions[0].message);
}

#[test]
fn resource_binding_feeds_state_json_and_skips_unchanged_inputs() {
    let registry = test_registry();
    let echo_node = EngineTestPlugin::new().echo.clone().alias("echo");
    let graph = registry
        .graph_builder()
        .expect("graph builder")
        .host_bridge("host")
        .node(&echo_node)
        .connect(&host_port("host", "sensor"), &echo_node.inputs.inp)
        .connect(&echo_node.outputs.out, &host_port("host", "result"))
        .build();
    let document = registry.graph_document(graph).to_json().expect("document json");
    let gpio = ResourceRecord::builder("gpio_line.node-local.0", "gpio.line", "provider.peripherals.node-local")
        .state(ResourceState::new(42).with_action_result(ResourceActionResult {
            action_kind: "gpio.read".into(),
            status: ResourceActionStatus::Read,
            data: Some(TypedConfigValue::Bool(true)),
            error: None,
        }))
        .build();
    let state = state_with(vec![gpio]);
    let workloads = [workload("workload.gpio", document, vec![binding("sensor", "gpio_line.node-local.0")])];
    let plugins = loaded_plugins();
    let plugins = ExecutionPlugins { registry: &registry, loaded_plugins: &plugins };
    let mut resident = ResidentExecutionSet::default();

    let first = resident.tick_workloads(&plugins, &workloads, Some(&state), 100);
    assert_eq!(first.sessions[0].status, ExecutionSessionStatus::Running, "{:?}", first.sessions[0].message);
    assert!(first.artifacts.iter().any(|artifact| artifact.message.as_deref().is_some_and(|message| message.contains("gpio.read"))));

    let second = resident.tick_workloads(&plugins, &workloads, Some(&state), 200);
    assert!(second.sessions[0].message.as_deref().is_some_and(|message| message.contains("\"skipped_unchanged_inputs\":true")));
    assert!(second.artifacts.is_empty());

    let missing = resident.tick_workloads(&plugins, &workloads, None, 300);
    assert_eq!(missing.sessions[0].status, ExecutionSessionStatus::Failed);
}

#[test]
fn workload_fails_when_required_plugin_is_missing() {
    let registry = test_registry();
    let mut missing = workload("workload.missing-plugin", ARUCO_GRAPH_DOCUMENT.into(), Vec::new());
    missing.plugin_requirements = vec![PluginRequirement { plugin_name: "not.loaded".into(), version: None }];

    let mut resident = ResidentExecutionSet::default();
    let snapshot = resident.tick_workloads(&ExecutionPlugins { registry: &registry, loaded_plugins: &loaded_plugins() }, &[missing], None, 42);

    assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Failed);
    assert!(snapshot.sessions[0].message.as_deref().is_some_and(|message| message.contains("not loaded")));
}

#[test]
fn frame_driver_pushes_context_with_each_frame() {
    let registry = test_registry();
    let fusion = EngineTestPlugin::new().frame_context.clone().alias("fusion");
    let graph = registry
        .graph_builder()
        .expect("graph builder")
        .host_bridge("host")
        .node(&fusion)
        .connect(&host_port("host", "camera"), &fusion.inputs.frame)
        .connect(&host_port("host", "imu"), &fusion.inputs.context)
        .connect(&fusion.outputs.out, &host_port("host", "result"))
        .build();
    let document = registry.graph_document(graph).to_json().expect("document json");
    let (mut driver, frames) = spawn_channel_driver(&registry, workload("workload.fusion", document, Vec::new()), "camera");

    driver.update_context(vec![bindings::ResourceInput { input: "imu".into(), revision: "1".into(), payload: "imu-a".into() }]);
    frames.send(grey_frame(&GrayImage::filled(8, 8, 0), 900)).expect("send frame");
    wait_for("first frame", || driver.stats().frames_processed == 1);
    let (session, artifacts) = driver.snapshot();
    assert_eq!(session.status, ExecutionSessionStatus::Running);
    let result = artifacts.iter().find(|artifact| artifact.kind == "host_output:result").and_then(|artifact| artifact.message.clone()).expect("result");
    assert!(result.contains("imu-a") && result.contains("\"timestamp\":900"), "{result}");

    driver.update_context(vec![bindings::ResourceInput { input: "imu".into(), revision: "2".into(), payload: "imu-b".into() }]);
    frames.send(grey_frame(&GrayImage::filled(8, 8, 0), 901)).expect("send frame");
    wait_for("second frame", || driver.stats().frames_processed == 2);
    let (_, artifacts) = driver.snapshot();
    let result = artifacts.iter().find(|artifact| artifact.kind == "host_output:result").and_then(|artifact| artifact.message.clone()).expect("result");
    assert!(result.contains("imu-b") && result.contains("\"timestamp\":901"), "{result}");
}

#[test]
fn frame_outputs_are_described_not_copied() {
    let registry = test_registry();
    let pass = EngineTestPlugin::new().frame_passthrough.clone().alias("pass");
    let graph = registry
        .graph_builder()
        .expect("graph builder")
        .host_bridge("host")
        .node(&pass)
        .connect(&host_port("host", "camera"), &pass.inputs.frame)
        .connect(&pass.outputs.frame, &host_port("host", "processed"))
        .build();
    let document = registry.graph_document(graph).to_json().expect("document json");
    let (mut driver, frames) = spawn_channel_driver(&registry, workload("workload.pass", document, Vec::new()), "camera");

    frames.send(grey_frame(&GrayImage::filled(4, 2, 0), 314)).expect("send frame");
    wait_for("frame", || driver.stats().frames_processed == 1);
    let (_, artifacts) = driver.snapshot();
    let processed = artifacts.iter().find(|artifact| artifact.kind == "host_output:processed").and_then(|artifact| artifact.message.clone()).expect("processed");
    let processed: serde_json::Value = serde_json::from_str(&processed).expect("frame descriptor json");
    assert_eq!(processed["type"], "styx:framelease");
    assert_eq!(processed["timestamp"], 314);
    assert_eq!(processed["fourcc"], "GREY");
}

/// The aruco GraphDocument, compiled with the vision plugin and driven by GREY frames through
/// the frame driver; markers are published as structured JSON.
#[test]
fn aruco_graph_document_detects_markers_through_the_frame_driver() {
    let registry = test_registry();
    let mut aruco = workload("workload.aruco", ARUCO_GRAPH_DOCUMENT.into(), Vec::new());
    aruco.plugin_requirements = vec![PluginRequirement { plugin_name: "helios.vision".into(), version: None }];
    let (mut driver, frames) = spawn_channel_driver(&registry, aruco, "frame");

    let scene = marker_scene();
    for timestamp in 1..=3 {
        frames.send(grey_frame(&scene, timestamp)).expect("send frame");
    }
    wait_for("three frames", || driver.stats().frames_processed == 3);
    let (session, artifacts) = driver.snapshot();
    assert_eq!(session.status, ExecutionSessionStatus::Running);
    assert_markers_artifact(&artifacts);
    let telemetry = artifacts.iter().find(|artifact| artifact.kind == "execution.telemetry").and_then(|artifact| artifact.message.clone()).expect("telemetry");
    let telemetry: serde_json::Value = serde_json::from_str(&telemetry).expect("telemetry json");
    assert_eq!(telemetry["frames_processed"], 3);
    assert_eq!(telemetry["last_frame_timestamp"], 3);
    assert!(telemetry["last_tick_ms"].as_f64().is_some_and(|ms| ms > 0.0), "{telemetry}");
}

fn assert_markers_artifact(artifacts: &[ExecutionArtifactRecord]) {
    let markers = artifacts.iter().find(|artifact| artifact.kind == "host_output:markers").expect("markers artifact");
    assert_eq!(markers.artifact_id, format!("{}.markers", markers.session_id));
    let markers: serde_json::Value = serde_json::from_str(markers.message.as_deref().expect("markers message")).expect("markers are JSON");
    let list = markers["markers"].as_array().expect("markers array");
    assert_eq!(list.len(), 1, "{markers}");
    assert_eq!(list[0]["id"], 17);
    assert_eq!(list[0]["dictionary"], "4x4_50");
    assert_eq!(list[0]["corners"].as_array().map(Vec::len), Some(4));
    assert!(list[0]["center"]["x"].is_number());
}

/// The vision plugin built as a dylib (`cargo build -p helios-vision --features dylib`), loaded
/// through the engine's plugin loader. Runs when `HELIOS_VISION_PLUGIN` names the `.so`.
#[test]
fn vision_dylib_loads_and_detects_markers() {
    let Some(path) = std::env::var_os("HELIOS_VISION_PLUGIN") else {
        eprintln!("HELIOS_VISION_PLUGIN not set; skipping the dylib plugin test");
        return;
    };
    let loaded = crate::plugins::load_plugins(&[path.into()]).expect("load vision dylib");
    let metadata = loaded.libraries.iter().map(|library| library.metadata().clone()).collect::<Vec<_>>();
    assert_eq!(metadata[0].plugin_name.as_deref(), Some("helios.vision"));
    let mut aruco = workload("workload.aruco-dylib", ARUCO_GRAPH_DOCUMENT.into(), Vec::new());
    aruco.plugin_requirements = vec![PluginRequirement { plugin_name: "helios.vision".into(), version: None }];
    let graph = graph::compile_workload_graph(&loaded.registry, &metadata, &aruco).expect("compile with dylib plugin");
    let (sender, receiver) = mpsc::channel();
    let mut driver = FrameDrivenExecution::spawn(aruco, graph, vec![frame_spec("frame")], vec![Box::new(ChannelFrameSource(receiver))]).expect("spawn driver");

    sender.send(grey_frame(&marker_scene(), 1)).expect("send frame");
    wait_for("dylib frame", || driver.stats().frames_received == 1);
    let stats = driver.stats();
    assert_eq!(stats.frames_processed, 1, "{stats:?}");
    let (_, artifacts) = driver.snapshot();
    assert_markers_artifact(&artifacts);
}

/// End to end over a real Styx camera service: a virtual camera served on a socket, bound
/// through a `styx-frames+unix://` resource endpoint and driven by `tick_workloads`.
#[test]
fn camera_service_drives_workload_through_resource_endpoint() {
    use styx::{
        capture_api::{CaptureRequest, VirtualSourceConfig},
        ipc::CameraService,
    };

    let temp = tempfile::tempdir().expect("tempdir");
    let socket = temp.path().join("camera.sock");
    let camera = CaptureRequest::virtual_source(VirtualSourceConfig::new().name("virtual-front").format(FourCc::GREY).resolution(320, 180).fps(60)).into_device();
    let service = CameraService::new(camera).serve(&socket).expect("serve camera");

    let registry = test_registry();
    let mut camera_resource = ResourceRecord::builder("camera.node-local.front", "camera.device", "provider.peripherals.node-local").build();
    camera_resource.endpoints = vec![format!("{STYX_FRAMES_ENDPOINT_SCHEME}://{}", socket.display())];
    let state = state_with(vec![camera_resource]);
    let mut frame_binding = binding("frame", "camera.node-local.front");
    frame_binding.frame_request.output_resolution = Some((160, 90));
    let workloads = [workload("workload.camera", ARUCO_GRAPH_DOCUMENT.into(), vec![frame_binding])];
    let plugins = loaded_plugins();
    let plugins = ExecutionPlugins { registry: &registry, loaded_plugins: &plugins };
    let mut resident = ResidentExecutionSet::default();

    let mut last = ExecutionSnapshot::default();
    wait_for("camera frames to be processed", || {
        last = resident.tick_workloads(&plugins, &workloads, Some(&state), 0);
        last.sessions[0].status == ExecutionSessionStatus::Running
    });
    let telemetry = last.artifacts.iter().find(|artifact| artifact.kind == "execution.telemetry").and_then(|artifact| artifact.message.clone()).expect("telemetry");
    let telemetry: serde_json::Value = serde_json::from_str(&telemetry).expect("telemetry json");
    assert_eq!(telemetry["source_connected"], true, "{telemetry}");
    assert!(telemetry["last_frame_width"].is_u64(), "{telemetry}");
    // The requested output size reaches the service's planner (this build's virtual camera
    // route cannot scale, so the plan says so and serves native frames).
    assert!(telemetry["source_plan"].as_str().is_some_and(|plan| plan.contains("160x90")), "{telemetry}");
    assert!(last.artifacts.iter().any(|artifact| artifact.kind == "host_output:markers"), "{:?}", last.artifacts);
    assert_eq!(service.stats().clients, 1);

    // Removing the workload stops its driver and closes its camera connection.
    let stopped_at = Instant::now();
    let removed = resident.tick_workloads(&plugins, &[], Some(&state), 0);
    assert!(removed.sessions.is_empty());
    assert!(stopped_at.elapsed() < Duration::from_secs(2));
    wait_for("the camera client to disconnect", || service.stats().clients == 0);
}
