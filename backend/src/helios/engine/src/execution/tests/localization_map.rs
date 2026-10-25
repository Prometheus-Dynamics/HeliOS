//! The localization-and-map templates: the live map graph (`live_map_builder`) with the AprilTag
//! templates' tag pose and multi-tag field pose on its detector, so one pipeline gives the robot's
//! field pose and the live map (and, with the dense mapper, its mesh) from one detector on one
//! core. Everything Eidos does for a camera shares that core.

use daedalus::{
    data::{model::Value, to_value::ToValue},
    runtime::{graph_builder::GraphBuilder, handles::PortHandle, plugins::PluginRegistry},
};

use super::{dense_map::with_dense_map, live_map::live_map_builder, *};

/// The tag pose node (`eidos:aruco.pose`).
const POSE_NODE: &str = "eidos:aruco.pose";
/// The multi-tag pose node (`eidos:aruco.multi_tag_pose`).
const MULTI_TAG_POSE_NODE: &str = "eidos:aruco.multi_tag_pose";

/// The localization-and-map graph as a stored document. `layout`'s tags are the multi-tag pose's
/// known tags (helios-api swaps in the selected field layout when it deploys); `dense` adds the
/// dense mapper. Host outputs: the live map's (`detections`, `pose`, `snapshot`, `timing`), the
/// tag poses (`poses`, `pose_solutions`), the field pose (`multi_tag_pose`) and, with `dense`,
/// `mesh` and `dense_timing`.
pub(super) fn localization_map_document(registry: &PluginRegistry, layout: &FieldLayout, dense: bool) -> String {
    let mut builder = with_pose_tails(live_map_builder(registry), layout);
    if dense {
        builder = with_dense_map(builder);
    }
    let mut document = registry.graph_document(builder.build());
    let title = if dense { "HeliOS localization, live map and dense map" } else { "HeliOS localization and live map" };
    document.metadata.insert("title".into(), title.into());
    stored_form(&document)
}

/// `builder`'s `detector` with the tag pose and the multi-tag pose after it, wired as Eidos's
/// template API wires them (`eidos_daedalus::templates`, its private `pose_tails`): the refined
/// corners, the held `camera` and `extrinsics` host inputs, every config field as a constant.
fn with_pose_tails(builder: GraphBuilder, layout: &FieldLayout) -> GraphBuilder {
    let detector = |port: &str| PortHandle::new("detector", port);
    let pose = |port: &str| PortHandle::new("pose", port);
    let multi = |port: &str| PortHandle::new("multi_tag_pose", port);
    let builder = builder
        .try_node_id(POSE_NODE, "pose")
        .expect("pose node")
        .try_connect(&detector("detections"), &pose("detections"))
        .expect("detections into the pose")
        .try_connect(&pose("poses"), "poses")
        .expect("poses output")
        .try_connect(&pose("solutions"), "pose_solutions")
        .expect("pose solutions output")
        .try_connect(&detector("refined_corners"), &pose("refined_corners"))
        .expect("refined corners into the pose")
        .try_connect("camera", &pose("camera"))
        .expect("camera into the pose");
    let builder = set_config(builder, "pose", &PoseConfig { tag_size_m: FRC_TAG_SIDE_M, ..PoseConfig::default() });
    let builder = builder
        .try_node_id(MULTI_TAG_POSE_NODE, "multi_tag_pose")
        .expect("multi-tag pose node")
        .try_connect(&detector("detections"), &multi("detections"))
        .expect("detections into the multi-tag pose")
        .try_connect(&multi("camera_pose"), "multi_tag_pose")
        .expect("multi-tag pose output")
        .try_connect(&detector("refined_corners"), &multi("refined_corners"))
        .expect("refined corners into the multi-tag pose")
        .const_input(&multi("known_tags"), Some(KnownTagPoses::from_known_tags(&layout.known_tags()).to_value()));
    let builder = set_config(builder, "multi_tag_pose", &MultiTagPoseConfig::default());
    builder
        .try_connect("camera", &multi("camera"))
        .expect("camera into the multi-tag pose")
        .try_connect("extrinsics", &multi("extrinsics"))
        .expect("extrinsics into the multi-tag pose")
}

/// Every field of a node config as a constant on node `alias`'s port of the same name.
fn set_config(mut builder: GraphBuilder, alias: &str, config: &impl ToValue) -> GraphBuilder {
    if let Value::Struct(fields) = config.to_value() {
        for field in fields {
            builder = builder.const_input(&PortHandle::new(alias, &field.name), Some(field.value));
        }
    }
    builder
}

/// Both localization-and-map templates through the frame driver, each one pipeline: the scene's
/// marker gets a tag pose from the shared detector while the live mapper tracks it, and the dense
/// template's dense mapper reports on the same ticks.
#[test]
fn localization_map_templates_pose_tags_and_map_in_one_pipeline() {
    use super::live_map::{SCENE_FOCAL_PX, json_with_keys, live_map_scene};

    let plugins = TestPlugins::load();
    let calibrated = calibration_context(SCENE_FOCAL_PX, "pinhole");
    for dense in [false, true] {
        let document = localization_map_document(plugins.registry(), &FieldLayout::frc_2026_andymark(), dense);
        let mut workload_spec = workload("workload.localization-map", document, vec![camera_binding(&calibrated)]);
        workload_spec.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
        let spec = workload_spec.clone();
        let (mut driver, frames) = spawn_channel_driver(&plugins, workload_spec, "frame", GraphSettings::default());
        driver.update_camera_context(&spec);

        let frame_count = 40_u64;
        for index in 0..frame_count {
            frames.send(live_map_scene(586, index, (index + 1) * 100_000_000)).expect("send frame");
            wait_for("the frame to be processed", || driver.stats().ticks_processed == index + 1);
        }
        assert!(driver.stats().last_error.is_none(), "dense {dense}: {:?}", driver.stats().last_error);
        let (session, artifacts) = driver.snapshot();
        assert_eq!(session.status, ExecutionSessionStatus::Running, "dense {dense}: {:?}", session.message);

        let poses = artifact_json(&artifacts, "host_output:poses");
        assert!(poses.to_string().contains("586"), "dense {dense}: the scene's tag has a pose: {poses}");
        let pose = json_with_keys(&artifacts, "host_output:pose", &["frame", "state", "valid"]);
        assert_eq!(pose["state"], "tracking", "dense {dense}: {pose}");
        if dense {
            json_with_keys(&artifacts, "host_output:dense_timing", &["state", "processed", "keyframes"]);
        }
    }
}
