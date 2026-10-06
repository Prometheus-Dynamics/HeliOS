use std::{
    num::NonZeroU32,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use daedalus::{
    declare_plugin,
    engine::MetricsLevel,
    host_bridge::host_port,
    macros::node,
    planner::HostInputPolicy,
    runtime::{NodeError, plugins::RegistryPluginExt},
};
use eidos_aruco::{ArucoDictionaryKind, BitGrid, dictionary};
use eidos_daedalus::{
    aruco::Dictionary,
    templates::{DetectorTemplate, detector_document},
};
use orion::control_plane::{ClusterStateEnvelope, ObservedClusterState, ResourceActionResult, ResourceActionStatus, ResourceRecord, ResourceState, TypedConfigValue};
use styx::{
    core::prelude::{BufferPool, ColorSpace, FourCc, FrameMeta, MediaFormat, Resolution, plane_layout_from_dims},
    imports::framelease::FrameLease,
};
use tokio::sync::{Notify, mpsc};

use super::{
    bindings::{FrameSourceSpec, ResourceInput},
    frame_source::{FrameReceive, FrameSource, FrameSourceStatus},
    *,
};
use crate::{
    model::{ExecutionBinding, FrameRequestOptions, GraphRef, PluginRequirement},
    plugins::{PluginLoadResult, load_plugins},
};

/// The stored graph documents engine workloads run: Eidos's detector templates.
const APRILTAG_GRAPH_DOCUMENT: &str = include_str!("../../graphs/apriltag-36h11.graph.json");
const ARUCO_GRAPH_DOCUMENT: &str = include_str!("../../graphs/aruco-4x4_50.graph.json");

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

#[node(id = "test.frame_pair", inputs("primary", "secondary"), outputs("out"))]
fn frame_pair(primary: &FrameLease, secondary: Option<&FrameLease>) -> Result<String, NodeError> {
    Ok(serde_json::json!({ "primary": primary.meta().timestamp, "secondary": secondary.map(|frame| frame.meta().timestamp) }).to_string())
}

declare_plugin!(EngineTestPlugin, "engine.test", [source, echo, frame_passthrough, frame_context, frame_pair]);

/// Frames handed over in-process, standing in for a camera service socket (pollable like a
/// Styx `FrameClient`).
struct ChannelFrameSource(mpsc::UnboundedReceiver<FrameLease>);

impl FrameSource for ChannelFrameSource {
    fn connect(&mut self) -> Result<(), (String, Duration)> {
        Ok(())
    }

    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<FrameReceive> {
        self.0.poll_recv(cx).map(|frame| frame.map_or_else(|| FrameReceive::Closed("test channel closed".into()), FrameReceive::Frame))
    }

    fn try_frame(&mut self) -> FrameReceive {
        self.0.try_recv().map_or(FrameReceive::Idle, FrameReceive::Frame)
    }

    fn status(&self) -> FrameSourceStatus {
        FrameSourceStatus { connected: true, ..FrameSourceStatus::default() }
    }
}

/// `libhelios_eidos_plugin.so`: `HELIOS_EIDOS_PLUGIN`, else the one `cargo test` built next to
/// this test binary (it is a dev-dependency, so it comes from the same build).
fn eidos_plugin_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HELIOS_EIDOS_PLUGIN") {
        return PathBuf::from(path);
    }
    let name = format!("{}helios_eidos_plugin{}", std::env::consts::DLL_PREFIX, std::env::consts::DLL_SUFFIX);
    let exe = std::env::current_exe().expect("test binary path");
    exe.ancestors()
        .skip(1)
        .take(2)
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("{name} not found next to {}; build it in the same cargo invocation (`cargo test -p helios-engine` does) or set HELIOS_EIDOS_PLUGIN", exe.display()))
}

/// The engine's plugin set, as in production (Styx frames, the built-ins, the Eidos plugin
/// library), plus the test nodes.
struct TestPlugins {
    loaded: PluginLoadResult,
    metadata: Vec<LoadedPlugin>,
}

impl TestPlugins {
    fn load() -> Self {
        let mut loaded = load_plugins(&[eidos_plugin_path()]).expect("load the Eidos plugin library");
        loaded.registry.install_plugin(&EngineTestPlugin::new()).expect("install test plugin");
        let mut metadata = loaded.builtins.clone();
        metadata.extend(loaded.libraries.iter().map(|library| library.metadata().clone()));
        metadata.push(LoadedPlugin { path: "<test>".into(), plugin_name: Some("engine.test".into()), plugin_version: None, daedalus_version: None });
        Self { loaded, metadata }
    }

    fn registry(&self) -> &PluginRegistry {
        &self.loaded.registry
    }

    fn execution(&self) -> ExecutionPlugins<'_> {
        ExecutionPlugins { registry: &self.loaded.registry, loaded_plugins: &self.metadata }
    }
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

fn grey_frame(width: u32, height: u32, pixels: &[u8], timestamp: u64) -> FrameLease {
    let format = MediaFormat::new(FourCc::from_str("GREY").expect("fourcc"), Resolution::new(width, height).expect("resolution"), ColorSpace::Srgb);
    let layout = plane_layout_from_dims(NonZeroU32::new(width).expect("width"), NonZeroU32::new(height).expect("height"), 1);
    let pool = BufferPool::lazy(layout.len, 1);
    let mut frame = FrameLease::single_plane(FrameMeta::new(format, timestamp), pool.lease(), layout.len, layout.stride);
    frame.copy_slice_to_visible_plane(0, pixels).expect("copy pixels");
    frame
}

fn flat_frame(width: u32, height: u32, timestamp: u64) -> FrameLease {
    grey_frame(width, height, &vec![0; (width * height) as usize], timestamp)
}

/// A 640x480 GREY scene with marker `id` of `kind`, drawn from Eidos's bit patterns.
fn marker_scene(kind: ArucoDictionaryKind, id: u32, timestamp: u64) -> FrameLease {
    let (width, height, cell, left, top) = (640_usize, 480_usize, 12_usize, 220_usize, 140_usize);
    let dict = dictionary(kind);
    let grid = BitGrid::from_marker(dict, id, 0).expect("marker bits");
    let cells = usize::from(dict.marker_size) + 2;
    let mut pixels = vec![200_u8; width * height];
    for cy in 0..cells {
        for cx in 0..cells {
            let border = cx == 0 || cy == 0 || cx == cells - 1 || cy == cells - 1;
            let value = if !border && grid.bit(cx - 1, cy - 1) { 235 } else { 20 };
            for y in top + cy * cell..top + (cy + 1) * cell {
                pixels[y * width + left + cx * cell..y * width + left + (cx + 1) * cell].fill(value);
            }
        }
    }
    grey_frame(width as u32, height as u32, &pixels, timestamp)
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Sync `workloads` until `done` accepts the snapshot (drivers run on their own threads).
fn sync_until(
    resident: &mut ResidentExecutionSet,
    plugins: &ExecutionPlugins<'_>,
    workloads: &[ExecutionWorkload],
    state: Option<&StateSnapshot>,
    what: &str,
    mut done: impl FnMut(&ExecutionSnapshot) -> bool,
) -> ExecutionSnapshot {
    let mut last = ExecutionSnapshot::default();
    wait_for(what, || {
        last = resident.sync_workloads(plugins, workloads, state, 42);
        done(&last)
    });
    last
}

fn artifact<'a>(snapshot: &'a ExecutionSnapshot, kind: &str) -> Option<&'a ExecutionArtifactRecord> {
    snapshot.artifacts.iter().find(|artifact| artifact.kind == kind)
}

fn artifact_json(artifacts: &[ExecutionArtifactRecord], kind: &str) -> serde_json::Value {
    let message = artifacts.iter().find(|artifact| artifact.kind == kind).and_then(|artifact| artifact.message.clone()).unwrap_or_else(|| panic!("{kind} artifact"));
    serde_json::from_str(&message).unwrap_or_else(|error| panic!("{kind} is JSON: {error}: {message}"))
}

fn frame_spec(input: &str) -> FrameSourceSpec {
    FrameSourceSpec { input: input.into(), resource_id: "camera.test".into(), socket_path: "/nonexistent.sock".into(), request: FrameRequestOptions::default() }
}

/// Start a driver for `workload` fed through the returned sender.
fn spawn_channel_driver(plugins: &TestPlugins, workload: ExecutionWorkload, input: &str, settings: GraphSettings) -> (WorkloadDriver, mpsc::UnboundedSender<FrameLease>) {
    let graph = graph::compile_workload_graph(plugins.registry(), &plugins.metadata, &workload, &[input.to_string()], settings).expect("compile graph");
    let (sender, receiver) = mpsc::unbounded_channel();
    let driver = WorkloadDriver::start(workload, graph, vec![(frame_spec(input), Box::new(ChannelFrameSource(receiver)) as Box<dyn FrameSource>)], Arc::new(Notify::new())).expect("start driver");
    (driver, sender)
}

#[test]
fn eidos_plugin_library_installs_through_the_rust_abi() {
    let plugins = TestPlugins::load();
    let eidos = plugins.loaded.libraries.iter().find(|library| library.metadata().plugin_name.as_deref() == Some("eidos")).expect("eidos plugin loaded");
    assert!(eidos.metadata().path.ends_with(eidos_plugin_path().file_name().expect("file name")));
    assert_eq!(eidos.install_path(), daedalus::dylib::InstallPath::RustAbi);
    // The AprilTag document's `requires` names it, and it provides every node.
    let document = daedalus::planner::GraphDocument::from_json(APRILTAG_GRAPH_DOCUMENT).expect("document");
    plugins.registry().check_document(&document).expect("requires are installed");
}

#[test]
fn graph_documents_are_eidos_templates() {
    let plugins = TestPlugins::load();
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for (file, template, stored) in [
        ("graphs/apriltag-36h11.graph.json", DetectorTemplate::apriltag(), APRILTAG_GRAPH_DOCUMENT),
        ("graphs/aruco-4x4_50.graph.json", DetectorTemplate::aruco(Dictionary::Aruco4x4_50), ARUCO_GRAPH_DOCUMENT),
    ] {
        let document = detector_document(plugins.registry(), &template).expect("template document");
        let json = document.to_json_pretty().expect("document json");
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(manifest_dir.join(file), format!("{json}\n")).expect("write golden");
            continue;
        }
        let expected: serde_json::Value = serde_json::from_str(stored).expect("stored document");
        let actual: serde_json::Value = serde_json::from_str(&json).expect("template json");
        assert_eq!(actual, expected, "{file} differs from Eidos's template; regenerate with UPDATE_GOLDEN=1 cargo test -p helios-engine graph_documents_are_eidos_templates");
    }
}

#[test]
fn inline_graph_document_without_inputs_runs_once() {
    let plugins = TestPlugins::load();
    let src = EngineTestPlugin::new().source.clone().alias("src");
    let graph = plugins.registry().graph_builder().expect("graph builder").host_bridge("host").node(&src).connect(&src.outputs.out, &host_port("host", "result")).build();
    let document = plugins.registry().graph_document(graph).to_json().expect("document json");
    assert!(document.contains("\"engine.test\""), "requires is filled from the registry: {document}");

    let mut resident = ResidentExecutionSet::default();
    let workloads = [workload("workload.inline", document, Vec::new())];
    let snapshot = sync_until(&mut resident, &plugins.execution(), &workloads, None, "the graph to run", |snapshot| snapshot.sessions[0].status == ExecutionSessionStatus::Running);
    let output = artifact(&snapshot, "host_output:result").expect("result artifact");
    assert_eq!(output.message.as_deref(), Some("{\"value\":7}"));
    assert_eq!(output.artifact_id, "session.workload.inline.result");

    std::thread::sleep(Duration::from_millis(50));
    let again = resident.sync_workloads(&plugins.execution(), &workloads, None, 43);
    assert_eq!(artifact_json(&again.artifacts, "execution.telemetry")["ticks_processed"], 1, "a graph without inputs runs once");
}

#[test]
fn bare_graph_json_is_rejected() {
    let plugins = TestPlugins::load();
    let src = EngineTestPlugin::new().source.clone().alias("src");
    let graph = plugins.registry().graph_builder().expect("graph builder").host_bridge("host").node(&src).connect(&src.outputs.out, &host_port("host", "result")).build();
    let bare = serde_json::to_string(&graph).expect("graph json");

    let mut resident = ResidentExecutionSet::default();
    let snapshot = resident.sync_workloads(&plugins.execution(), &[workload("workload.bare", bare, Vec::new())], None, 42);

    assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Failed);
    assert!(snapshot.sessions[0].message.as_deref().is_some_and(|message| message.contains("GraphDocument")), "{:?}", snapshot.sessions[0].message);
}

#[test]
fn document_requires_must_name_the_plugins_of_its_nodes() {
    let plugins = TestPlugins::load();
    let mut document = daedalus::planner::GraphDocument::from_json(APRILTAG_GRAPH_DOCUMENT).expect("document");
    document.requires.clear();
    let json = document.to_json().expect("json");

    let mut resident = ResidentExecutionSet::default();
    let snapshot = resident.sync_workloads(&plugins.execution(), &[workload("workload.no-requires", json, Vec::new())], None, 42);

    assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Failed);
    assert!(snapshot.sessions[0].message.as_deref().is_some_and(|message| message.contains("`requires` does not list plugin(s) eidos")), "{:?}", snapshot.sessions[0].message);
}

#[test]
fn resource_binding_ticks_when_the_resource_changes() {
    let plugins = TestPlugins::load();
    let echo_node = EngineTestPlugin::new().echo.clone().alias("echo");
    let graph = plugins
        .registry()
        .graph_builder()
        .expect("graph builder")
        .host_bridge("host")
        .node(&echo_node)
        .connect(&host_port("host", "sensor"), &echo_node.inputs.inp)
        .connect(&echo_node.outputs.out, &host_port("host", "result"))
        .build();
    let document = plugins.registry().graph_document(graph).to_json().expect("document json");
    let gpio = |value: bool, observed_at_ms: u64| {
        ResourceRecord::builder("gpio_line.node-local.0", "gpio.line", "provider.peripherals.node-local")
            .state(ResourceState::new(observed_at_ms).with_action_result(ResourceActionResult {
                action_kind: "gpio.read".into(),
                status: ResourceActionStatus::Read,
                data: Some(TypedConfigValue::Bool(value)),
                error: None,
            }))
            .build()
    };
    let workloads = [workload("workload.gpio", document, vec![binding("sensor", "gpio_line.node-local.0")])];
    let execution = plugins.execution();
    let mut resident = ResidentExecutionSet::default();
    let ticks = |snapshot: &ExecutionSnapshot| artifact_json(&snapshot.artifacts, "execution.telemetry")["ticks_processed"].as_u64().unwrap_or_default();

    let state = state_with(vec![gpio(true, 42)]);
    let first = sync_until(&mut resident, &execution, &workloads, Some(&state), "the first tick", |snapshot| snapshot.sessions[0].status == ExecutionSessionStatus::Running);
    let result = artifact(&first, "host_output:result").and_then(|artifact| artifact.message.clone()).expect("result");
    assert!(result.contains("gpio.read") && result.contains("true"), "{result}");

    // Unchanged resources do not run the graph again.
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(ticks(&resident.sync_workloads(&execution, &workloads, Some(&state), 43)), 1);

    let changed = state_with(vec![gpio(false, 43)]);
    let second = sync_until(&mut resident, &execution, &workloads, Some(&changed), "the second tick", |snapshot| ticks(snapshot) == 2);
    let result = artifact(&second, "host_output:result").and_then(|artifact| artifact.message.clone()).expect("result");
    assert!(result.contains("false"), "{result}");

    let missing = resident.sync_workloads(&execution, &workloads, None, 44);
    assert_eq!(missing.sessions[0].status, ExecutionSessionStatus::Failed);
}

#[test]
fn workload_fails_when_required_plugin_is_missing() {
    let plugins = TestPlugins::load();
    let mut missing = workload("workload.missing-plugin", APRILTAG_GRAPH_DOCUMENT.into(), Vec::new());
    missing.plugin_requirements = vec![PluginRequirement { plugin_name: "not.loaded".into(), version: None }];

    let mut resident = ResidentExecutionSet::default();
    let snapshot = resident.sync_workloads(&plugins.execution(), &[missing], None, 42);

    assert_eq!(snapshot.sessions[0].status, ExecutionSessionStatus::Failed);
    assert!(snapshot.sessions[0].message.as_deref().is_some_and(|message| message.contains("not loaded")));
}

#[test]
fn frame_driver_pushes_context_with_each_frame() {
    let plugins = TestPlugins::load();
    let fusion = EngineTestPlugin::new().frame_context.clone().alias("fusion");
    let graph = plugins
        .registry()
        .graph_builder()
        .expect("graph builder")
        .host_bridge("host")
        .node(&fusion)
        .connect(&host_port("host", "camera"), &fusion.inputs.frame)
        .connect(&host_port("host", "imu"), &fusion.inputs.context)
        .connect(&fusion.outputs.out, &host_port("host", "result"))
        .build();
    let document = plugins.registry().graph_document(graph).to_json().expect("document json");
    let (mut driver, frames) = spawn_channel_driver(&plugins, workload("workload.fusion", document, Vec::new()), "camera", GraphSettings::default());
    let result = |driver: &mut WorkloadDriver| driver.snapshot().1.into_iter().find(|artifact| artifact.kind == "host_output:result").and_then(|artifact| artifact.message).expect("result");

    driver.update_context(vec![ResourceInput { input: "imu".into(), revision: "1".into(), payload: "imu-a".into() }]);
    frames.send(flat_frame(8, 8, 900)).expect("send frame");
    wait_for("first frame", || driver.stats().ticks_processed == 1);
    assert_eq!(driver.snapshot().0.status, ExecutionSessionStatus::Running);
    let first = result(&mut driver);
    assert!(first.contains("imu-a") && first.contains("\"timestamp\":900"), "{first}");

    // The context alone does not run a frame-driven graph; the next frame sees the new one.
    driver.update_context(vec![ResourceInput { input: "imu".into(), revision: "2".into(), payload: "imu-b".into() }]);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(driver.stats().ticks_processed, 1);
    frames.send(flat_frame(8, 8, 901)).expect("send frame");
    wait_for("second frame", || driver.stats().ticks_processed == 2);
    let second = result(&mut driver);
    assert!(second.contains("imu-b") && second.contains("\"timestamp\":901"), "{second}");
}

/// Context inputs of a frame-driven graph are held in the document before it is planned
/// (Daedalus's `GraphDocument::set_host_input_policy`); frame inputs stay queued.
#[test]
fn context_inputs_are_declared_held_before_planning() {
    let plugins = TestPlugins::load();
    let fusion = EngineTestPlugin::new().frame_context.clone().alias("fusion");
    let graph = plugins
        .registry()
        .graph_builder()
        .expect("graph builder")
        .host_bridge("host")
        .node(&fusion)
        .connect(&host_port("host", "camera"), &fusion.inputs.frame)
        .connect(&host_port("host", "imu"), &fusion.inputs.context)
        .connect(&fusion.outputs.out, &host_port("host", "result"))
        .build();
    let mut document = plugins.registry().graph_document(graph);
    graph::declare_context_held(&mut document, "host", &["camera".to_string()]).expect("declare held");
    assert_eq!(document.host_input_policy("host", "imu").expect("imu policy"), HostInputPolicy::Held);
    assert_eq!(document.host_input_policy("host", "camera").expect("camera policy"), HostInputPolicy::Queued);
}

/// A secondary camera's latest frame goes in one batch with each primary frame (the primary paces
/// the graph), on the graph thread: no feeder thread per camera.
#[test]
fn secondary_camera_frames_pair_with_the_primary_frame() {
    let plugins = TestPlugins::load();
    let pair = EngineTestPlugin::new().frame_pair.clone().alias("pair");
    let graph = plugins
        .registry()
        .graph_builder()
        .expect("graph builder")
        .host_bridge("host")
        .node(&pair)
        .connect(&host_port("host", "front"), &pair.inputs.primary)
        .connect(&host_port("host", "side"), &pair.inputs.secondary)
        .connect(&pair.outputs.out, &host_port("host", "result"))
        .build();
    let document = plugins.registry().graph_document(graph).to_json().expect("document json");
    let workload = workload("workload.pair", document, Vec::new());
    let compiled = graph::compile_workload_graph(plugins.registry(), &plugins.metadata, &workload, &["front".to_string(), "side".to_string()], GraphSettings::default()).expect("compile graph");
    let (front, front_frames) = mpsc::unbounded_channel();
    let (side, side_frames) = mpsc::unbounded_channel();
    let sources =
        vec![(frame_spec("front"), Box::new(ChannelFrameSource(front_frames)) as Box<dyn FrameSource>), (frame_spec("side"), Box::new(ChannelFrameSource(side_frames)) as Box<dyn FrameSource>)];
    let mut driver = WorkloadDriver::start(workload, compiled, sources, Arc::new(Notify::new())).expect("start driver");
    let result = |driver: &mut WorkloadDriver| artifact_json(&driver.snapshot().1, "host_output:result");

    // A side frame alone does not tick the graph; the next front frame carries it.
    side.send(flat_frame(4, 4, 50)).expect("send side");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(driver.stats().ticks_processed, 0);
    front.send(flat_frame(4, 4, 100)).expect("send front");
    wait_for("the first pair", || driver.stats().ticks_processed == 1);
    assert_eq!(result(&mut driver), serde_json::json!({ "primary": 100, "secondary": 50 }));

    // The side camera's newest frame is reused until a newer one arrives.
    front.send(flat_frame(4, 4, 101)).expect("send front");
    wait_for("the second pair", || driver.stats().ticks_processed == 2);
    assert_eq!(result(&mut driver), serde_json::json!({ "primary": 101, "secondary": 50 }));
    side.send(flat_frame(4, 4, 51)).expect("send side");
    side.send(flat_frame(4, 4, 52)).expect("send side");
    front.send(flat_frame(4, 4, 102)).expect("send front");
    wait_for("the third pair", || driver.stats().ticks_processed == 3);
    assert_eq!(result(&mut driver), serde_json::json!({ "primary": 102, "secondary": 52 }));
    assert_eq!(driver.stats().frames_received, 3);
}

#[test]
fn frame_outputs_are_described_not_copied() {
    let plugins = TestPlugins::load();
    let pass = EngineTestPlugin::new().frame_passthrough.clone().alias("pass");
    let graph = plugins
        .registry()
        .graph_builder()
        .expect("graph builder")
        .host_bridge("host")
        .node(&pass)
        .connect(&host_port("host", "camera"), &pass.inputs.frame)
        .connect(&pass.outputs.frame, &host_port("host", "processed"))
        .build();
    let document = plugins.registry().graph_document(graph).to_json().expect("document json");
    let (mut driver, frames) = spawn_channel_driver(&plugins, workload("workload.pass", document, Vec::new()), "camera", GraphSettings::default());

    frames.send(flat_frame(4, 2, 314)).expect("send frame");
    wait_for("frame", || driver.stats().ticks_processed == 1);
    let processed = artifact_json(&driver.snapshot().1, "host_output:processed");
    assert_eq!(processed["type"], "styx:framelease");
    assert_eq!(processed["timestamp"], 314);
    assert_eq!(processed["fourcc"], "GREY");
}

/// The stored AprilTag `GraphDocument`, run by the Eidos plugin library and driven by GREY frames
/// through the frame driver: detections are published as structured JSON, with the plan (host
/// ports, adapter edges) and the per-node metrics. `HELIOS_PRINT_PLAN=1` prints the plan.
#[test]
fn apriltag_graph_detects_markers_through_the_frame_driver() {
    let plugins = TestPlugins::load();
    let mut apriltag = workload("workload.apriltag", APRILTAG_GRAPH_DOCUMENT.into(), Vec::new());
    apriltag.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    let (mut driver, frames) = spawn_channel_driver(&plugins, apriltag, "frame", GraphSettings { metrics_level: MetricsLevel::Detailed });

    for timestamp in 1..=3 {
        frames.send(marker_scene(ArucoDictionaryKind::AprilTag36h11, 586, timestamp)).expect("send frame");
        wait_for("the frame to be processed", || driver.stats().ticks_processed == timestamp);
    }
    let (session, artifacts) = driver.snapshot();
    assert_eq!(session.status, ExecutionSessionStatus::Running);

    let detections = artifact_json(&artifacts, "host_output:detections");
    let list = detections["detections"].as_array().expect("detections array");
    assert_eq!(list.len(), 1, "{detections}");
    assert_eq!(list[0]["id"], 586);
    assert_eq!(list[0]["corners"].as_array().map(Vec::len), Some(4));
    assert!(artifacts.iter().any(|artifact| artifact.kind == "host_output:refined_corners"));

    let telemetry = artifact_json(&artifacts, "execution.telemetry");
    assert_eq!(telemetry["frames_received"], 3);
    assert_eq!(telemetry["ticks_processed"], 3);
    assert_eq!(telemetry["last_frame_timestamp"], 3);

    let plan = artifact_json(&artifacts, "execution.plan");
    if std::env::var_os("HELIOS_PRINT_PLAN").is_some() {
        eprintln!("{}", serde_json::to_string_pretty(&plan).expect("plan json"));
    }
    assert_eq!(plan["format"], graph::PLAN_FORMAT);
    assert_eq!(plan["host_inputs"][0]["name"], "frame");
    assert_eq!(plan["host_inputs"][0]["type_key"], "styx:framelease");
    let outputs = plan["host_outputs"].as_array().expect("host outputs").iter().map(|port| port["name"].as_str().unwrap_or_default().to_string()).collect::<Vec<_>>();
    assert!(outputs.contains(&"detections".to_string()) && outputs.contains(&"refined_corners".to_string()), "{outputs:?}");
    assert!(plan["plan"]["nodes"].as_array().is_some_and(|nodes| nodes.len() >= 5), "{plan}");
    assert!(plan["adapter_edges"].is_array());
    assert_eq!(plan["requires"][0]["id"], "eidos");

    let metrics = artifact_json(&artifacts, "execution.metrics");
    assert_eq!(metrics["metrics_level"], "Detailed");
    let nodes = metrics["total"]["nodes"].as_array().expect("node metrics");
    for stage in ["mask_prep", "quads", "decode", "validate", "refine"] {
        assert!(nodes.iter().any(|node| node["label"] == stage && node["calls"] == 3), "{stage} metrics in {metrics}");
    }
    assert_eq!(metrics["total"]["ticks"], 3);
    // Daedalus's frame-path overhead rides along whenever metrics are on.
    assert_eq!(plan["frame_overhead"], true);
    let overhead = &metrics["frame_overhead"];
    assert!(overhead["recorded"].as_u64().is_some_and(|recorded| recorded >= 1), "{overhead}");
    assert!(overhead["stages"].as_array().is_some_and(|stages| stages.iter().any(|stage| stage["name"] == "tick")), "{overhead}");
    assert!(plan["copying_edges"].is_array() && plan["crossing_edges"].is_array(), "{plan}");
    assert!(plan["plan"]["edges"].as_array().is_some_and(|edges| edges.iter().all(|edge| edge["copies_frame"].is_boolean())), "{plan}");
}

/// End to end over a real Styx camera service: a virtual camera served on a socket, bound
/// through a `styx-frames+unix://` resource endpoint and driven by `sync_workloads`.
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

    let plugins = TestPlugins::load();
    let mut camera_resource = ResourceRecord::builder("camera.node-local.front", "camera.device", "provider.peripherals.node-local").build();
    camera_resource.endpoints = vec![format!("{STYX_FRAMES_ENDPOINT_SCHEME}://{}", socket.display())];
    let state = state_with(vec![camera_resource]);
    let mut frame_binding = binding("frame", "camera.node-local.front");
    frame_binding.frame_request.output_resolution = Some((160, 90));
    let workloads = [workload("workload.camera", ARUCO_GRAPH_DOCUMENT.into(), vec![frame_binding])];
    let execution = plugins.execution();
    let mut resident = ResidentExecutionSet::default();

    let last = sync_until(&mut resident, &execution, &workloads, Some(&state), "camera frames to be processed", |snapshot| snapshot.sessions[0].status == ExecutionSessionStatus::Running);
    let telemetry = artifact_json(&last.artifacts, "execution.telemetry");
    assert_eq!(telemetry["source_connected"], true, "{telemetry}");
    assert!(telemetry["last_frame_width"].is_u64(), "{telemetry}");
    // The requested output size reaches the service's planner (this build's virtual camera
    // route cannot scale, so the plan says so and serves native frames).
    assert!(telemetry["source_plan"].as_str().is_some_and(|plan| plan.contains("160x90")), "{telemetry}");
    assert!(last.artifacts.iter().any(|artifact| artifact.kind == "host_output:detections"), "{:?}", last.artifacts);
    assert_eq!(service.stats().clients, 1);

    // Removing the workload stops its driver and closes its camera connection.
    let stopped_at = Instant::now();
    let removed = resident.sync_workloads(&execution, &[], Some(&state), 0);
    assert!(removed.sessions.is_empty());
    assert!(stopped_at.elapsed() < Duration::from_secs(2));
    wait_for("the camera client to disconnect", || service.stats().clients == 0);
}
