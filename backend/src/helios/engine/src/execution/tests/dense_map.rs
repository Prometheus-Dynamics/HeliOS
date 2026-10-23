//! The dense mapper template (`eidos:dense.map`), fed by a live mapper pipeline: the camera's
//! frames and calibration come from the camera binding, and the pose of each frame from the live
//! mapper's `pose` output on the pipeline bus.

use daedalus::{
    data::model::Value,
    runtime::{graph_builder::GraphBuilder, handles::PortHandle, plugins::PluginRegistry},
};
use eidos_daedalus::{mapping::MapPose, types::CameraCalibration};

use super::{
    live_map::{SCENE_FOCAL_PX, base64_decoded_len, json_with_keys, live_map_scene, live_map_template},
    *,
};
use crate::model::PipelineInputSource;

/// The dense mapper node (`eidos:dense.map`).
const DENSE_MAP_NODE: &str = "eidos:dense.map";

/// The dense graph as a stored document: the host's `frame` feeds the mapper, its `pose` (a held
/// host input, the live mapper's `pose` output, bound to the live-map pipeline) and `camera` (the
/// calibration of the camera context, held) feed the mapper's `pose` and `calibration`. Host
/// outputs: `mesh` and `timing`. `tuning` sets the mapper's constants (empty for the defaults).
pub(super) fn dense_map_document(registry: &PluginRegistry, tuning: &[(&str, Value)]) -> String {
    let dense = |port: &str| PortHandle::new("dense", port);
    let mut builder = registry
        .graph_builder()
        .expect("graph builder")
        .input_typed::<FrameLease>("frame")
        .expect("frame input")
        .input_typed::<CameraCalibration>("camera")
        .expect("camera input")
        .held_input("camera")
        .input_typed::<MapPose>("pose")
        .expect("pose input")
        .held_input("pose")
        .try_node_id(DENSE_MAP_NODE, "dense")
        .expect("dense map node")
        .try_connect("frame", &dense("frame"))
        .expect("frame into the dense map")
        .try_connect("pose", &dense("pose"))
        .expect("pose into the dense map")
        .try_connect("camera", &dense("calibration"))
        .expect("camera into the dense map")
        .try_connect(&dense("mesh"), "mesh")
        .expect("mesh output")
        .try_connect(&dense("timing"), "timing")
        .expect("timing output");
    for (port, value) in tuning {
        builder = set_const(builder, port, value.clone());
    }
    let mut document = registry.graph_document(builder.build());
    document.metadata.insert("title".into(), "HeliOS dense map".into());
    stored_form(&document)
}

fn set_const(builder: GraphBuilder, port: &str, value: Value) -> GraphBuilder {
    builder.const_input(&PortHandle::new("dense", port), Some(value))
}

/// The dense mapper's settings for a short test clip: a keyframe every second posed frame, from
/// the frames before it (causal: no wait for later neighbours), and the default rest.
fn short_clip_tuning() -> Vec<(&'static str, Value)> {
    vec![("key_every", Value::Int(2)), ("window", Value::Int(40)), ("causal", Value::Bool(true))]
}

/// The dense mapper through the frame driver, fed the live mapper's poses through the pipeline bus:
/// every frame ticks once, aligned to the live mapper's pose of the same frame; the mapper reports
/// it is running (posed), and its keyframes give a mesh whose byte buffers match its counts.
#[test]
fn dense_map_takes_the_live_map_pose_of_each_frame_through_the_frame_driver() {
    let plugins = TestPlugins::load();
    let bus = PipelineBus::default();
    let calibration = calibration_context(SCENE_FOCAL_PX, "pinhole");

    let mut live = workload("pipeline.live-map", live_map_template(plugins.registry()), vec![camera_binding(&calibration)]);
    live.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    let live_spec = live.clone();
    let (mut live_driver, live_frames) = start_pipeline(&plugins, &bus, live, "frame");

    let mut dense = workload(
        "pipeline.dense-map",
        dense_map_document(plugins.registry(), &short_clip_tuning()),
        vec![
            camera_binding(&calibration),
            ExecutionBinding { input: "pose".into(), pipeline: Some(PipelineInputSource { pipeline: "live-map".into(), port: "pose".into(), max_wait_ms: 2000 }), ..ExecutionBinding::default() },
        ],
    );
    dense.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    let dense_spec = dense.clone();
    let (mut dense_driver, dense_frames) = start_pipeline(&plugins, &bus, dense, "frame");
    dense_driver.update_camera_context(&dense_spec);
    live_driver.update_camera_context(&live_spec);

    // Both pipelines get the same frame; the mapper's frame waits for the live mapper's pose of it.
    let frame_count = 24_u64;
    for index in 0..frame_count {
        let timestamp = (index + 1) * 100_000_000;
        dense_frames.send(live_map_scene(586, index, timestamp)).expect("send dense frame");
        live_frames.send(live_map_scene(586, index, timestamp)).expect("send live frame");
        wait_for("the dense mapper to process the frame", || dense_driver.stats().ticks_processed == index + 1);
    }
    assert!(dense_driver.stats().last_error.is_none(), "{:?}", dense_driver.stats().last_error);

    let feeds = dense_driver.stats().pipeline_inputs;
    assert_eq!(feeds.len(), 1, "{feeds:?}");
    assert_eq!((feeds[0].aligned_ticks, feeds[0].lagged_ticks), (frame_count, 0), "every frame met its own pose: {feeds:?}");

    let (session, artifacts) = dense_driver.snapshot();
    assert_eq!(session.status, ExecutionSessionStatus::Running, "{:?}", session.message);
    let timing = json_with_keys(&artifacts, "host_output:timing", &["state", "processed", "keyframes", "pending", "sweep_ms", "integrate_ms", "mesh_ms", "submaps", "volume_bytes"]);
    assert_eq!(timing["state"], "running", "the mapper has a pose for its frames: {timing}");
    assert!(timing["keyframes"].as_u64().is_some_and(|keyframes| keyframes >= 1), "keyframes were integrated: {timing}");

    let mesh = json_with_keys(&artifacts, "host_output:mesh", &["seq", "frame", "t_ns", "keyframes", "submaps", "voxel_m", "vertices", "triangles", "positions", "normals", "grey", "indices"]);
    let vertices = mesh["vertices"].as_u64().expect("vertex count") as usize;
    let triangles = mesh["triangles"].as_u64().expect("triangle count") as usize;
    assert!(vertices > 0 && triangles > 0, "the mesh has surface: {mesh}");
    assert_eq!(base64_decoded_len(mesh["positions"].as_str().expect("positions")), vertices * 12, "f32 xyz per vertex");
    assert_eq!(base64_decoded_len(mesh["normals"].as_str().expect("normals")), vertices * 12, "f32 xyz per vertex");
    assert_eq!(base64_decoded_len(mesh["indices"].as_str().expect("indices")), triangles * 3 * 4, "u32 per corner");
    let grey = base64_decoded_len(mesh["grey"].as_str().expect("grey"));
    assert!(grey == 0 || grey == vertices, "grey is one byte per vertex or empty: {grey}");
}

/// Start a frame-driven pipeline on `bus` (its frames from the returned test camera).
fn start_pipeline(plugins: &TestPlugins, bus: &PipelineBus, workload: ExecutionWorkload, frame_input: &str) -> (WorkloadDriver, TestCamera) {
    let graph = graph::compile_workload_graph(plugins.registry(), &plugins.metadata, &workload, &[frame_input.to_string()], GraphSettings::default()).expect("compile graph");
    let (sender, receiver) = test_camera();
    let driver = WorkloadDriver::start(workload, graph, vec![(frame_spec(frame_input), Box::new(receiver) as Box<dyn FrameSource>)], Arc::new(Notify::new()), crate::nt4::Nt4Sink::default(), bus.clone())
        .expect("start driver");
    (driver, sender)
}

#[test]
fn dense_map_template_takes_the_pose_from_a_pipeline_and_the_frames_from_the_camera() {
    let plugins = TestPlugins::load();
    let document = dense_map_document(plugins.registry(), &[]);
    let graph: serde_json::Value = serde_json::from_str(&document).expect("json");
    let requires = graph["requires"].as_array().expect("requires");
    assert!(requires.iter().any(|requirement| requirement["id"] == "eidos"), "{requires:?}");
    assert!(document.contains(DENSE_MAP_NODE), "the dense map node");
    for port in ["frame", "pose", "camera"] {
        assert!(document.contains(&format!("\"{port}\"")), "host input {port}");
    }
}
