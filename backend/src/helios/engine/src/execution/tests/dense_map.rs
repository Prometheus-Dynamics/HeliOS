//! The dense mapper template (`eidos:dense.map`): the live map graph with the dense mapper on the
//! live mapper's pose, in one pipeline. Everything Eidos does for a camera shares one core, so the
//! dense mapper runs in budget mode (its default: a time budget per tick, keyframe spacing adapted
//! to what the budget sustains) instead of on a core of its own.

use daedalus::runtime::plugins::PluginRegistry;
use eidos_daedalus::dense::DenseConfig;

use super::{
    live_map::{SCENE_FOCAL_PX, base64_decoded_len, json_with_keys, live_map_scene},
    *,
};

/// The dense graph as a stored document (`localization_map::map_document`): the live map graph
/// with the dense mapper on the mapper's pose. Host outputs: the live map's and the dense mapper's
/// `dense_mesh`, `dense_timing` and `dense_depth` (its inverse-depth preview). `config` is the dense
/// mapper's settings (`DenseConfig::default()` for its defaults, budget mode included).
pub(super) fn dense_map_document(registry: &PluginRegistry, config: DenseConfig) -> String {
    super::localization_map::map_document(registry, None, Some(config))
}

/// The dense mapper's settings for a short test clip: a keyframe every second posed frame, from
/// the frames before it (causal: no wait for later neighbours), whole keyframes per tick (no
/// budget, so the clip's keyframes all land), and the default rest.
fn short_clip_tuning() -> DenseConfig {
    DenseConfig { key_every: 2, window: 40, causal: true, budget_ms: 0.0, ..DenseConfig::default() }
}

/// The dense template through the frame driver, one pipeline: the live mapper poses each frame and
/// the dense mapper reports it is running (posed); its keyframes give a mesh whose byte buffers
/// match its counts.
#[test]
fn dense_map_maps_the_live_map_pose_in_one_pipeline() {
    let plugins = TestPlugins::load();
    let calibration = calibration_context(SCENE_FOCAL_PX, "pinhole");
    let mut dense = workload("pipeline.dense-map", dense_map_document(plugins.registry(), short_clip_tuning()), vec![camera_binding(&calibration)]);
    dense.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    let spec = dense.clone();
    let (mut driver, frames) = start_pipeline(&plugins, &PipelineBus::default(), dense, "frame");
    driver.update_camera_context(&spec);

    let frame_count = 24_u64;
    for index in 0..frame_count {
        frames.send(live_map_scene(586, index, (index + 1) * 100_000_000)).expect("send frame");
        wait_for("the pipeline to process the frame", || driver.stats().ticks_processed == index + 1);
    }
    assert!(driver.stats().last_error.is_none(), "{:?}", driver.stats().last_error);

    let (session, artifacts) = driver.snapshot();
    assert_eq!(session.status, ExecutionSessionStatus::Running, "{:?}", session.message);
    let timing = json_with_keys(&artifacts, "host_output:dense_timing", &["state", "processed", "keyframes", "pending", "sweep_ms", "integrate_ms", "mesh_ms", "submaps", "volume_bytes"]);
    assert_eq!(timing["state"], "running", "the live mapper posed the frames: {timing}");
    assert!(timing["keyframes"].as_u64().is_some_and(|keyframes| keyframes >= 1), "keyframes were integrated: {timing}");

    let mesh = json_with_keys(&artifacts, "host_output:dense_mesh", &["seq", "frame", "t_ns", "keyframes", "submaps", "voxel_m", "vertices", "triangles", "positions", "normals", "grey", "indices"]);
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
fn dense_map_template_is_the_live_map_with_the_dense_mapper() {
    let plugins = TestPlugins::load();
    let document = dense_map_document(plugins.registry(), DenseConfig::default());
    let graph: serde_json::Value = serde_json::from_str(&document).expect("json");
    let requires = graph["requires"].as_array().expect("requires");
    assert!(requires.iter().any(|requirement| requirement["id"] == "eidos"), "{requires:?}");
    for node in ["eidos:dense.map", "eidos:mapping.live_map", "eidos:detectors.tracked_detect"] {
        assert!(document.contains(node), "{node} in the one pipeline");
    }
}
