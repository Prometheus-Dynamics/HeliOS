//! The ArUco graph, compiled from a versioned graph document and driven with
//! a camera-like frame.

use std::{num::NonZeroU32, str::FromStr, sync::Arc};

use daedalus::{
    data::model::TypeExpr,
    engine::{Engine, EngineConfig},
    planner::GraphDocument,
    runtime::plugins::PluginRegistry,
    transport::{Payload, Residency, TypeKey},
};
use helios_vision::{
    aruco::{
        DICT_4X4_50,
        render::{paste_warped, render_marker},
    },
    image::GrayImage,
    plugin::{FRAMELEASE_TYPE_KEY, MarkerList, VisionPlugin},
};
use styx::{
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

fn aruco_document(registry: &PluginRegistry, plugin: &VisionPlugin) -> GraphDocument {
    let threshold = plugin.adaptive_threshold.clone().alias("threshold");
    let quads = plugin.find_quads.clone().alias("quads");
    let decode = plugin.decode.clone().alias("decode");
    let graph = registry
        .graph_builder()
        .unwrap()
        .input_as("frame", TypeExpr::opaque(FRAMELEASE_TYPE_KEY))
        .try_node(&threshold)
        .unwrap()
        .try_node(&quads)
        .unwrap()
        .try_node(&decode)
        .unwrap()
        .try_connect("frame", &threshold.inputs.gray)
        .unwrap()
        .try_connect("frame", &decode.inputs.gray)
        .unwrap()
        .try_connect(&threshold.outputs.binary, &quads.inputs.binary)
        .unwrap()
        .try_connect(&quads.outputs.quads, &decode.inputs.quads)
        .unwrap()
        .try_connect(&decode.outputs.markers, "markers")
        .unwrap()
        .build();
    registry.graph_document(graph)
}

#[test]
fn aruco_graph_detects_marker_in_a_camera_frame() {
    let plugin = VisionPlugin::new();
    let mut registry = PluginRegistry::new();
    registry.install(&plugin).expect("install vision plugin");

    // Round-trip through the versioned JSON form the engine receives.
    let json = aruco_document(&registry, &plugin).to_json().expect("serialize document");
    assert!(json.contains("\"daedalus.graph\""), "{json}");
    let document = GraphDocument::from_json(&json).expect("parse document");

    let mut host = Engine::new(EngineConfig::default()).unwrap().compile_document(&registry, document).expect("compile");
    host.set_latest_input("frame").unwrap();

    let mut scene = GrayImage::filled(640, 480, 140);
    let marker = render_marker(&DICT_4X4_50, 17, 16, 1).unwrap();
    assert!(paste_warped(&mut scene, &marker, [[200.0, 120.0], [400.0, 140.0], [390.0, 330.0], [190.0, 310.0]]));
    let frame = grey_frame(&scene);
    let bytes = frame.payload_bytes() as u64;
    let payload = Payload::shared_with(TypeKey::new(FRAMELEASE_TYPE_KEY), Arc::new(frame), Residency::Cpu, None, Some(bytes));
    host.push_payload("frame", payload);
    host.tick().expect("tick");

    let markers: MarkerList = host.take("markers").expect("markers output");
    assert_eq!(markers.markers.len(), 1, "{markers:?}");
    assert_eq!(markers.markers[0].id, 17);
    assert_eq!(markers.markers[0].dictionary, "4x4_50");
    assert_eq!(markers.markers[0].corners.len(), 4);
}
