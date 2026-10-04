//! Ready-made vision graphs.

use daedalus::{data::model::TypeExpr, data::to_value::ToValue, planner::GraphDocument, runtime::plugins::PluginRegistry};

use crate::plugin::{FRAMELEASE_TYPE_KEY, VisionPlugin};

/// Host input that receives camera frames (`styx:framelease`).
pub const FRAME_INPUT: &str = "frame";
/// Host output that delivers `helios:aruco_markers`.
pub const MARKERS_OUTPUT: &str = "markers";

/// The ArUco/AprilTag graph: frame -> mask (half-size luma) -> quads ->
/// decode (full frame) -> refine corners -> markers. Every frame consumer
/// reads the frame in place.
pub fn aruco_graph_document(registry: &PluginRegistry, plugin: &VisionPlugin, dictionary: &str) -> Result<GraphDocument, Box<dyn std::error::Error + Send + Sync>> {
    let mask = plugin.aruco_mask.clone().alias("mask");
    let quads = plugin.find_quads.clone().alias("quads");
    let decode = plugin.decode.clone().alias("decode");
    let refine = plugin.refine_marker_corners.clone().alias("refine");
    let graph = registry
        .graph_builder()?
        .input_as(FRAME_INPUT, TypeExpr::opaque(FRAMELEASE_TYPE_KEY))
        .try_node(&mask)?
        .try_node(&quads)?
        .try_node(&decode)?
        .try_node(&refine)?
        .try_connect(FRAME_INPUT, &mask.inputs.frame)?
        .try_connect(&mask.outputs.mask, &quads.inputs.mask)?
        .try_connect(&quads.outputs.quads, &decode.inputs.quads)?
        .try_connect(FRAME_INPUT, &decode.inputs.frame)?
        .try_connect(&decode.outputs.markers, &refine.inputs.markers)?
        .try_connect(FRAME_INPUT, &refine.inputs.frame)?
        .try_connect(&refine.outputs.markers, MARKERS_OUTPUT)?
        .const_input_by_id("decode", "dictionary", Some(dictionary.to_value()))
        .build();
    Ok(registry.graph_document(graph))
}
