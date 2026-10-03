//! Ready-made vision graphs.

use daedalus::{data::model::TypeExpr, data::to_value::ToValue, planner::GraphDocument, runtime::plugins::PluginRegistry};

use crate::plugin::{FRAMELEASE_TYPE_KEY, VisionPlugin};

/// Host input that receives camera frames (`styx:framelease`).
pub const FRAME_INPUT: &str = "frame";
/// Host output that delivers `helios:aruco_markers`.
pub const MARKERS_OUTPUT: &str = "markers";

/// The ArUco detection graph: frame -> downscale -> adaptive threshold ->
/// quads -> decode -> markers. Quads are searched on the downscaled image
/// and decoded on the full-resolution frame; the planner inserts the luma
/// adapter on each frame edge.
pub fn aruco_graph_document(registry: &PluginRegistry, plugin: &VisionPlugin, dictionary: &str) -> Result<GraphDocument, Box<dyn std::error::Error + Send + Sync>> {
    let downscale = plugin.downscale.clone().alias("downscale");
    let threshold = plugin.adaptive_threshold.clone().alias("threshold");
    let quads = plugin.find_quads.clone().alias("quads");
    let decode = plugin.decode.clone().alias("decode");
    let graph = registry
        .graph_builder()?
        .input_as(FRAME_INPUT, TypeExpr::opaque(FRAMELEASE_TYPE_KEY))
        .try_node(&downscale)?
        .try_node(&threshold)?
        .try_node(&quads)?
        .try_node(&decode)?
        .try_connect(FRAME_INPUT, &downscale.inputs.gray)?
        .try_connect(&downscale.outputs.gray, &threshold.inputs.gray)?
        .try_connect(FRAME_INPUT, &decode.inputs.gray)?
        .try_connect(&threshold.outputs.binary, &quads.inputs.binary)?
        .try_connect(&quads.outputs.quads, &decode.inputs.quads)?
        .try_connect(&decode.outputs.markers, MARKERS_OUTPUT)?
        .const_input_by_id("decode", "dictionary", Some(dictionary.to_value()))
        .build();
    Ok(registry.graph_document(graph))
}
