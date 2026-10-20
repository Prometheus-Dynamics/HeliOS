//! The feature-tracking template: one camera frame in, the live feature tracks out as `tracks`.
//! Eidos ships no template for `eidos:features.track`, so the graph is built with the graph
//! builder its own templates use and stored the way helios-api stores the others.

use daedalus::runtime::{handles::PortHandle, plugins::PluginRegistry};

use super::*;

/// The node the template runs (Eidos's features crate, `eidos_daedalus`).
const FEATURE_TRACKS_NODE: &str = "eidos:features.track";

/// `eidos:features.track` as a stored graph document: the host's `frame` input feeds the tracker
/// and its `tracks` output is the host output `tracks` (`eidos:feature_tracks`).
pub(super) fn feature_tracks_template(registry: &PluginRegistry) -> String {
    let graph = registry
        .graph_builder()
        .expect("graph builder")
        .input_typed::<FrameLease>("frame")
        .expect("frame input")
        .try_node_id(FEATURE_TRACKS_NODE, "tracker")
        .expect("features.track node")
        .try_connect("frame", &PortHandle::new("tracker", "frame"))
        .expect("frame into the tracker")
        .try_connect(&PortHandle::new("tracker", "tracks"), "tracks")
        .expect("tracks output")
        .build();
    let mut document = registry.graph_document(graph);
    document.metadata.insert("title".into(), "HeliOS feature tracks".into());
    stored_form(&document)
}

/// A GREY scene of 16 px checker squares, shifted `dx` pixels to the right: its corners move with
/// the scene, so the tracker has something to follow.
fn checker_scene(dx: usize, timestamp: u64) -> FrameLease {
    let (width, height, square) = (320_usize, 240_usize, 16_usize);
    let mut pixels = vec![0_u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let shifted = x + dx;
            let light = (shifted / square + y / square).is_multiple_of(2);
            pixels[y * width + x] = if light { 220 } else { 30 };
        }
    }
    grey_frame(width as u32, height as u32, &pixels, timestamp)
}

/// The feature-tracking template run by the Eidos plugin library through the frame driver: the
/// first frame's tracks are all new; after the scene moves, the same ids are followed (`tracked`,
/// with an age), and the summary counts agree with the list. The output is the structured
/// `eidos:feature_tracks` JSON, not a fallback description of the type.
#[test]
fn feature_tracks_graph_publishes_tracks_through_the_frame_driver() {
    let plugins = TestPlugins::load();
    let document = feature_tracks_template(plugins.registry());
    let mut workload_spec = workload("workload.features", document, Vec::new());
    workload_spec.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    let (mut driver, frames) = spawn_channel_driver(&plugins, workload_spec, "frame", GraphSettings::default());

    frames.send(checker_scene(0, 1)).expect("send frame");
    wait_for("the first frame", || driver.stats().ticks_processed == 1);
    let first = artifact_json(&driver.snapshot().1, "host_output:tracks");
    let first_tracks = first["tracks"].as_array().expect("tracks array");
    assert!(!first_tracks.is_empty(), "the checker corners are detected: {first}");
    assert!(first_tracks.iter().all(|track| track["status"] == "new" && track["age"] == 0), "{first}");
    assert_eq!(first["summary"]["count"].as_u64(), Some(first_tracks.len() as u64), "{first}");
    assert_eq!(first["summary"]["new"].as_u64(), Some(first_tracks.len() as u64), "{first}");

    frames.send(checker_scene(2, 2)).expect("send frame");
    wait_for("the second frame", || driver.stats().ticks_processed == 2);
    let second = artifact_json(&driver.snapshot().1, "host_output:tracks");
    let second_tracks = second["tracks"].as_array().expect("tracks array");
    let followed: Vec<&serde_json::Value> = second_tracks.iter().filter(|track| track["status"] == "tracked").collect();
    assert!(!followed.is_empty(), "the moved scene keeps tracks: {second}");
    assert!(followed.iter().all(|track| track["age"].as_u64().is_some_and(|age| age >= 1)), "{second}");
    let first_ids: Vec<i64> = first_tracks.iter().filter_map(|track| track["id"].as_i64()).collect();
    assert!(followed.iter().all(|track| track["id"].as_i64().is_some_and(|id| first_ids.contains(&id))), "followed ids were seen before: {second}");
    let summary = &second["summary"];
    assert_eq!(summary["count"].as_u64(), Some(second_tracks.len() as u64), "{second}");
    assert_eq!(summary["tracked"].as_u64(), Some(followed.len() as u64), "{second}");
    assert_eq!(summary["tracked"].as_u64().unwrap_or(0) + summary["new"].as_u64().unwrap_or(0), summary["count"].as_u64().unwrap_or(0), "{second}");
}
