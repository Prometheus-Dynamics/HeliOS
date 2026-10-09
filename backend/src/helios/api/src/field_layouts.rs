//! Field layouts: where the AprilTags are on the field, for the multi-tag (field) pose.
//!
//! A layout is a WPILib AprilTag field layout JSON or a Limelight `.fmap`, read and converted by
//! `helios_field` (HeliOS owns the FRC conventions; Eidos only solves against known tag poses).
//! The FRC 2026 AndyMark field is built in; uploaded layouts are kept in the API's state
//! directory (`field-layouts/<id>.json`). Each pipeline uses the layout it names
//! (`field_layout`), else the selected one (`field-layout.json`), else the built-in 2026 field:
//! the API writes that layout's tags into the `known_tags` constant of the graph's
//! `eidos:aruco.multi_tag_pose` nodes when it deploys the pipeline.

use helios_field::{FRC_2026_ANDYMARK_ID, FieldLayout, LayoutFormat};
use serde::{Deserialize, Serialize};

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
};

/// Eidos's multi-tag pose node, whose `known_tags` constant a layout fills.
pub const MULTI_TAG_POSE_NODE: &str = "eidos:aruco.multi_tag_pose";

/// A layout the API knows: built in, or uploaded and stored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredLayout {
    pub id: String,
    pub name: String,
    /// The file format it was read from.
    pub format: LayoutFormat,
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub saved_at_ms: u64,
    pub layout: FieldLayout,
}

impl StoredLayout {
    /// The summary the list shows.
    pub fn summary(&self, selected: &str) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "format": self.format,
            "builtin": self.builtin,
            "selected": self.id == selected,
            "saved_at_ms": self.saved_at_ms,
            "length_m": self.layout.length_m,
            "width_m": self.layout.width_m,
            "tags": self.layout.tags.len(),
            "tag_ids": self.layout.tags.iter().map(|tag| tag.id).collect::<Vec<_>>(),
        })
    }
}

/// The built-in layouts (parsed once).
pub fn builtin_layouts() -> Vec<StoredLayout> {
    static BUILTIN: std::sync::OnceLock<Vec<StoredLayout>> = std::sync::OnceLock::new();
    BUILTIN
        .get_or_init(|| {
            vec![StoredLayout {
                id: FRC_2026_ANDYMARK_ID.into(),
                name: "FRC 2026 REBUILT (AndyMark field)".into(),
                format: LayoutFormat::Fmap,
                builtin: true,
                saved_at_ms: 0,
                layout: FieldLayout::frc_2026_andymark(),
            }]
        })
        .clone()
}

/// Whether `id` is a built-in layout.
pub fn is_builtin(id: &str) -> bool {
    id == FRC_2026_ANDYMARK_ID
}

/// Every layout, the built-in ones first.
pub async fn all_layouts(state: &SharedState) -> ApiResult<Vec<StoredLayout>> {
    let mut layouts = builtin_layouts();
    layouts.extend(state.store.field_layouts().await?.into_iter().filter(|stored| !is_builtin(&stored.id)));
    Ok(layouts)
}

/// The id of the layout pipelines without their own get.
pub async fn selected_id(state: &SharedState) -> ApiResult<String> {
    Ok(state.store.selected_field_layout().await?.unwrap_or_else(|| FRC_2026_ANDYMARK_ID.to_string()))
}

/// The layout with `id`.
pub async fn layout(state: &SharedState, id: &str) -> ApiResult<StoredLayout> {
    all_layouts(state).await?.into_iter().find(|layout| layout.id == id).ok_or_else(|| ApiError::unprocessable(format!("no field layout {id}")))
}

/// The layout a pipeline gets: the one it names, else the selected one.
pub async fn layout_for(state: &SharedState, named: Option<&str>) -> ApiResult<StoredLayout> {
    match named {
        Some(id) => layout(state, id).await,
        None => {
            let selected = selected_id(state).await?;
            match layout(state, &selected).await {
                Ok(layout) => Ok(layout),
                // A selection whose file is gone falls back to the built-in field.
                Err(_) => Ok(builtin_layouts().remove(0)),
            }
        }
    }
}

/// Whether `graph` (a `GraphDocument` as JSON) has a multi-tag pose node.
pub fn has_multi_tag_pose(graph: &serde_json::Value) -> bool {
    graph.pointer("/graph/nodes").and_then(|n| n.as_array()).is_some_and(|nodes| nodes.iter().any(|node| node.get("id").and_then(|id| id.as_str()) == Some(MULTI_TAG_POSE_NODE)))
}

/// Write `layout`'s tags into the `known_tags` constant of every multi-tag pose node of `graph`.
/// Returns how many nodes it set.
pub fn apply_layout(graph: &mut serde_json::Value, layout: &FieldLayout) -> ApiResult<usize> {
    let constant = daedalus_data::json::from_plain_json(&layout.known_tags_value()).map_err(|error| ApiError::internal(format!("field layout as a graph value: {error}")))?;
    let constant = serde_json::to_value(constant).map_err(|error| ApiError::internal(error.to_string()))?;
    let Some(nodes) = graph.pointer_mut("/graph/nodes").and_then(|n| n.as_array_mut()) else {
        return Ok(0);
    };
    let mut set = 0;
    for node in nodes.iter_mut().filter(|node| node.get("id").and_then(|id| id.as_str()) == Some(MULTI_TAG_POSE_NODE)) {
        if node.get("const_inputs").and_then(|c| c.as_array()).is_none() {
            node["const_inputs"] = serde_json::json!([]);
        }
        let consts = node["const_inputs"].as_array_mut().expect("just ensured");
        consts.retain(|entry| entry.get(0).and_then(|name| name.as_str()) != Some("known_tags"));
        consts.push(serde_json::json!(["known_tags", constant.clone()]));
        set += 1;
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_layout_is_the_2026_field() {
        let builtin = builtin_layouts();
        assert_eq!(builtin[0].id, FRC_2026_ANDYMARK_ID);
        assert_eq!(builtin[0].layout.tags.len(), 32);
        let summary = builtin[0].summary(FRC_2026_ANDYMARK_ID);
        assert_eq!(summary["selected"], true);
        assert_eq!(summary["tags"], 32);
    }

    #[test]
    fn a_layout_fills_the_known_tags_constant() {
        let mut graph = serde_json::json!({ "graph": { "nodes": [
            { "id": "eidos:detectors.apriltag_tracked" },
            { "id": MULTI_TAG_POSE_NODE, "const_inputs": [["known_tags", { "type": "List", "value": [] }], ["joint_iterations", { "type": "Int", "value": 30 }]] },
        ] } });
        assert!(has_multi_tag_pose(&graph));
        assert_eq!(apply_layout(&mut graph, &FieldLayout::frc_2026_andymark()).expect("apply"), 1);
        let consts = graph.pointer("/graph/nodes/1/const_inputs").and_then(|c| c.as_array()).expect("consts");
        assert_eq!(consts.len(), 2, "replaced, not added: {consts:?}");
        let known = consts.iter().find(|entry| entry[0] == "known_tags").expect("known_tags");
        // A Daedalus value that reads back as the layout's 32 tags.
        let value: daedalus_data::model::Value = serde_json::from_value(known[1].clone()).expect("a Daedalus value");
        let plain = daedalus_data::json::to_plain_json(&value);
        assert_eq!(plain.as_array().map(Vec::len), Some(32));
        assert_eq!(plain[0]["id"], 1);
        assert!(!has_multi_tag_pose(&serde_json::json!({ "graph": { "nodes": [{ "id": "eidos:aruco.pose" }] } })));
    }
}
