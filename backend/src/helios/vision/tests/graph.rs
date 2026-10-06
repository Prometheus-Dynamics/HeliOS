//! The ArUco graph, compiled from a versioned graph document and driven with
//! a camera-like frame.

use std::{num::NonZeroU32, str::FromStr};

use daedalus::{
    engine::{Engine, EngineConfig},
    planner::GraphDocument,
    runtime::plugins::PluginRegistry,
};
use helios_vision::{
    graphs::aruco_graph_document,
    image::GrayImage,
    plugin::{MarkerList, VisionPlugin},
    testing::{
        DICT_4X4_50,
        render::{paste_warped, render_marker},
    },
};
use styx::{
    core::daedalus::{StyxFramesPlugin, frame_payload},
    core::prelude::{BufferPool, ColorSpace, FourCc, FrameMeta, MediaFormat, Resolution, plane_layout_from_dims},
    imports::framelease::FrameLease,
};

fn grey_frame(image: &GrayImage) -> FrameLease {
    let (w, h) = (image.width() as u32, image.height() as u32);
    let format = MediaFormat::new(FourCc::from_str("GREY").unwrap(), Resolution::new(w, h).unwrap(), ColorSpace::Srgb);
    let layout = plane_layout_from_dims(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap(), 1);
    let pool = BufferPool::lazy(layout.len, 1);
    let mut frame = FrameLease::single_plane(FrameMeta::new(format, 1), pool.lease(), layout.len, layout.stride);
    frame.copy_slice_to_visible_plane(0, image.data()).unwrap();
    frame
}

#[test]
fn aruco_graph_detects_marker_in_a_camera_frame() {
    let plugin = VisionPlugin::new();
    let mut registry = PluginRegistry::new();
    registry.install(&StyxFramesPlugin::new()).expect("install styx frames");
    registry.install(&plugin).expect("install vision plugin");

    // Round-trip through the versioned JSON form the engine receives.
    let json = aruco_graph_document(&registry, &plugin, "4x4_50").expect("build graph").to_json().expect("serialize document");
    assert!(json.contains("\"daedalus.graph\""), "{json}");
    let document = GraphDocument::from_json(&json).expect("parse document");

    let mut host = Engine::new(EngineConfig::default()).unwrap().compile_document(&registry, document).expect("compile");
    host.set_latest_input("frame").unwrap();

    let mut scene = GrayImage::filled(640, 480, 140);
    let marker = render_marker(&DICT_4X4_50, 17, 16, 1).unwrap();
    assert!(paste_warped(&mut scene, &marker, [[200.0, 120.0], [400.0, 140.0], [390.0, 330.0], [190.0, 310.0]]));
    let frame = grey_frame(&scene);
    let payload = frame_payload(frame);
    host.push_payload("frame", payload);
    host.tick().expect("tick");

    let markers: MarkerList = host.take("markers").expect("markers output");
    assert_eq!(markers.markers.len(), 1, "{markers:?}");
    assert_eq!(markers.markers[0].id, 17);
    assert_eq!(markers.markers[0].dictionary, "4x4_50");
    assert_eq!(markers.markers[0].corners.len(), 4);
}

/// `graphs/aruco-4x4_50.graph.json` is the document engine workloads use.
/// Regenerate it with `UPDATE_GOLDEN=1 cargo test -p helios-vision --test graph`.
#[test]
fn aruco_graph_document_matches_golden_file() {
    let plugin = VisionPlugin::new();
    let mut registry = PluginRegistry::new();
    registry.install(&StyxFramesPlugin::new()).expect("install styx frames");
    registry.install(&plugin).expect("install vision plugin");
    for dictionary in ["4x4_50", "36h11"] {
        let json = aruco_graph_document(&registry, &plugin, dictionary).expect("build graph").to_json().expect("serialize document");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("graphs/aruco-{dictionary}.graph.json"));
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("{json}\n")).unwrap();
        }
        let golden = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(golden.trim_end(), json, "{} is stale; regenerate with UPDATE_GOLDEN=1", path.display());
        GraphDocument::from_json(&golden).expect("golden document parses");
    }
}

#[test]
fn apriltag_graph_detects_36h11() {
    let plugin = VisionPlugin::new();
    let mut registry = PluginRegistry::new();
    registry.install(&StyxFramesPlugin::new()).expect("install styx frames");
    registry.install(&plugin).expect("install vision plugin");
    let document = aruco_graph_document(&registry, &plugin, "36h11").expect("build graph");
    let mut host = Engine::new(EngineConfig::default()).unwrap().compile_document(&registry, document).expect("compile");
    let mut scene = GrayImage::filled(640, 480, 120);
    let marker = render_marker(&helios_vision::testing::TAG_36H11, 586, 14, 1).unwrap();
    assert!(paste_warped(&mut scene, &marker, [[180.0, 100.0], [420.0, 110.0], [410.0, 360.0], [170.0, 350.0]]));
    let frame = grey_frame(&scene);
    host.push_payload("frame", frame_payload(frame));
    host.tick().expect("tick");
    let markers: MarkerList = host.take("markers").expect("markers output");
    assert_eq!(markers.markers.iter().map(|m| (m.dictionary.as_str(), m.id)).collect::<Vec<_>>(), vec![("36h11", 586)]);
}
