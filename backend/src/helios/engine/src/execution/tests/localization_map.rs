//! HeliOS's map templates, built with Eidos's tracked detector template
//! (`TrackedDetectorTemplate::with_live_map`, `with_dense_map`): the AprilTag 36h11 detector with
//! HeliOS's tracking feeding the live mapper, optionally with the tag pose and the multi-tag field
//! pose on the same detector (localization) and the dense mapper on the mapper's pose. Everything
//! Eidos does for a camera stays in one pipeline on one core. Host outputs are Eidos's names:
//! `detections`, `map_pose`, `map_snapshot`, `map_timing`, with localization `poses`,
//! `pose_solutions` and `multi_tag_pose`, with the dense mapper `dense_mesh` and `dense_timing`.

use eidos_daedalus::{dense::DenseConfig, mapping::MappingConfig};

use super::{
    live_map::{SCENE_FOCAL_PX, json_with_keys, live_map_scene},
    *,
};

/// A map template as a stored document. `localization`: the field layout whose tags the
/// multi-tag pose knows (helios-api swaps in the selected one when it deploys); `dense`: the dense
/// mapper's settings. The mapper takes the camera's extrinsics but no layout (a layout makes it
/// wait for a known tag; without one it starts on the first clear tag, the map's reference).
pub(super) fn map_document(registry: &PluginRegistry, localization: Option<&FieldLayout>, dense: Option<DenseConfig>) -> String {
    let mut template = TrackedDetectorTemplate {
        single_node: true,
        tracking: TrackingOverrides { loss_full_search_after: Some(0), margin_growth_misses: Some(6), ..TrackingOverrides::default() },
        camera: CameraInput::GraphInput,
        ..TrackedDetectorTemplate::new(Dictionary::AprilTag36h11, 8)
    }
    .with_live_map(MappingConfig { tag_size_m: FRC_TAG_SIDE_M, ..MappingConfig::default() });
    if let Some(live) = template.live_map.as_mut() {
        live.extrinsics = StructuredInput::GraphInput;
    }
    if let Some(layout) = localization {
        template.pose = Some(PoseConfig { tag_size_m: FRC_TAG_SIDE_M, ..PoseConfig::default() });
        template.multi_tag_pose = Some(MultiTagPoseTemplate { config: MultiTagPoseConfig::default(), known_tags: KnownTagPoses::from_known_tags(&layout.known_tags()), extrinsics: StructuredInput::GraphInput });
    }
    let title = match (localization.is_some(), dense.is_some()) {
        (false, false) => "HeliOS live map",
        (false, true) => "HeliOS dense map",
        (true, false) => "HeliOS localization and live map",
        (true, true) => "HeliOS localization, live map and dense map",
    };
    if let Some(config) = dense {
        template = template.with_dense_map(config);
    }
    let mut document = registry.graph_document(tracked_detector_graph(registry, &template).expect("map template graph"));
    document.metadata.insert("title".into(), title.into());
    stored_form(&document)
}

/// The localization-and-map templates (the AprilTag templates' tag and field pose on the live
/// map's detector), with or without the dense mapper.
pub(super) fn localization_map_document(registry: &PluginRegistry, layout: &FieldLayout, dense: bool) -> String {
    map_document(registry, Some(layout), dense.then(DenseConfig::default))
}

/// Both localization-and-map templates through the frame driver, each one pipeline: the scene's
/// marker gets a tag pose from the shared detector while the live mapper tracks it, and the dense
/// template's dense mapper reports on the same ticks.
#[test]
fn localization_map_templates_pose_tags_and_map_in_one_pipeline() {
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
        let pose = json_with_keys(&artifacts, "host_output:map_pose", &["frame", "state", "valid"]);
        assert_eq!(pose["state"], "tracking", "dense {dense}: {pose}");
        if dense {
            json_with_keys(&artifacts, "host_output:dense_timing", &["state", "processed", "keyframes"]);
        }
    }
}

/// The node catalog the UI reads carries the parameter metadata Eidos declares (Daedalus
/// `#[port(label, unit, group, advanced)]`): the tracked detector's settings have labels and
/// groups, and some are marked expert.
#[test]
fn catalog_carries_parameter_labels_groups_and_expert_flags() {
    let plugins = TestPlugins::load();
    let catalog = crate::plugins::catalog::catalog(plugins.registry());
    let detector = catalog["nodes"].as_array().expect("nodes").iter().find(|node| node["id"] == "eidos:detectors.tracked_detect").expect("tracked detector in the catalog");
    let settings: Vec<&serde_json::Value> = detector["inputs"].as_array().expect("inputs").iter().filter(|port| port.get("default").is_some()).collect();
    assert!(settings.iter().any(|port| port["label"].is_string()), "labels: {detector}");
    assert!(settings.iter().any(|port| port["group"].is_string()), "groups: {detector}");
    assert!(settings.iter().any(|port| port["advanced"] == true), "expert settings: {detector}");
}
