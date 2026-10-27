//! Saved maps in a live-map graph (`eidos:mapping.live_map`): the file a graph starts from and the
//! save request, both through the graph's host inputs.
//!
//! - **Load.** A workload with a map (`ExecutionWorkload::map`) starts its graph with the map
//!   file's bytes in the held `map` input, pushed before the graph thread runs, so the first tick
//!   already has it. The buffer is pushed once, as one shared `Arc`: the mapper compares it by
//!   identity, so a restart (a new driver) is a new run and nothing else is.
//! - **Save.** The `save` input is a bool, saved on its rising edge. A new save request (a new
//!   value of `ExecutionWorkload::map_save`) raises it; the node saves on the next tick, gives
//!   `saved_map` on that tick, and the driver lowers `save` then. Requests that arrive while one is
//!   pending share its edge. Lowering after each save is what makes the next request an edge.

use std::{path::Path, sync::Arc};

use eidos_daedalus::types::{BYTES_TYPE_KEY, Bytes};

/// The graph's held input a saved map is pushed into (`eidos:mapping.live_map`'s `map`).
pub(crate) const MAP_PORT: &str = "map";
/// The graph's bool input that requests a save (`save`).
pub(crate) const SAVE_PORT: &str = "save";
/// The graph's output with a save's map file (`saved_map`).
pub(crate) const SAVED_MAP_PORT: &str = "saved_map";
/// The type key the map input takes (`eidos:bytes`).
pub(crate) const MAP_TYPE_KEY: &str = BYTES_TYPE_KEY;

/// The bytes of the map file at `path`, shared.
pub(crate) fn load_map(path: &Path) -> Result<Arc<Bytes>, String> {
    std::fs::read(path).map(|bytes| Arc::new(Bytes { bytes })).map_err(|error| format!("map {}: {error}", path.display()))
}

const HOST_NODE: &str = "io.host_bridge";
const LIVE_MAP_NODE: &str = "eidos:mapping.live_map";
const HOST_HELD_INPUTS: &str = "daedalus.host_held_inputs";
const HOST_INPUT_TYPES: &str = "daedalus.host_input_types";

/// Adds a saved map's host ports to a live-map graph document (`GraphDocument` JSON), so the
/// graph can take a map and be asked to save one: `map` and `save` are held host outputs (the
/// host pushes them), `saved_map` a host input. Eidos's templates leave them out (the golden
/// graphs are Eidos's output), so HeliOS adds them where a graph is deployed. Idempotent: a graph
/// that has them is unchanged. `false` when the graph has no live map (nothing to wire).
pub fn wire_map_ports(graph: &mut serde_json::Value) -> bool {
    let Some(nodes) = graph.pointer("/graph/nodes").and_then(serde_json::Value::as_array) else { return false };
    let position = |id: &str| nodes.iter().position(|node| node.get("id").and_then(serde_json::Value::as_str) == Some(id));
    let (Some(host), Some(live)) = (position(HOST_NODE), position(LIVE_MAP_NODE)) else { return false };
    let nodes = graph.pointer_mut("/graph/nodes").and_then(serde_json::Value::as_array_mut).expect("nodes were just read");
    let host_node = &mut nodes[host];
    for port in ["map", "save"] {
        push_unique(&mut host_node["outputs"], port);
    }
    push_unique(&mut host_node["inputs"], "saved_map");
    let metadata = &mut host_node["metadata"];
    for port in ["map", "save"] {
        push_unique_string(metadata_value(metadata, HOST_HELD_INPUTS, "List"), port);
    }
    let types = metadata_value(metadata, HOST_INPUT_TYPES, "Map");
    push_type(types, "map", r#"{"Opaque":"eidos:bytes"}"#);
    push_type(types, "save", r#"{"Scalar":"Bool"}"#);

    let Some(edges) = graph.pointer_mut("/graph/edges").and_then(serde_json::Value::as_array_mut) else { return false };
    for (from, to) in [((host, "map"), (live, "map")), ((host, "save"), (live, "save")), ((live, "saved_map"), (host, "saved_map"))] {
        let from = serde_json::json!({ "node": from.0, "port": from.1 });
        let to = serde_json::json!({ "node": to.0, "port": to.1 });
        if !edges.iter().any(|edge| edge["from"] == from && edge["to"] == to) {
            edges.push(serde_json::json!({ "from": from, "to": to }));
        }
    }
    true
}

/// `value` (a JSON array of port names) with `port` appended, unless it has it.
fn push_unique(value: &mut serde_json::Value, port: &str) {
    if let Some(list) = value.as_array_mut()
        && !list.iter().any(|entry| entry.as_str() == Some(port))
    {
        list.push(serde_json::json!(port));
    }
}

/// The `{"type": "List", "value": [...]}` metadata entry `key`, made when missing.
fn metadata_value<'a>(metadata: &'a mut serde_json::Value, key: &str, kind: &str) -> &'a mut Vec<serde_json::Value> {
    if metadata[key]["value"].is_null() {
        metadata[key] = serde_json::json!({ "type": kind, "value": [] });
    }
    metadata[key]["value"].as_array_mut().expect("a list entry")
}

fn push_unique_string(list: &mut Vec<serde_json::Value>, port: &str) {
    if !list.iter().any(|entry| entry["value"] == port) {
        list.push(serde_json::json!({ "type": "String", "value": port }));
    }
}

/// The host input types entry for `port` (a `[port, type]` pair, the type as JSON text).
fn push_type(types: &mut Vec<serde_json::Value>, port: &str, type_json: &str) {
    if !types.iter().any(|pair| pair[0]["value"] == port) {
        types.push(serde_json::json!([{ "type": "String", "value": port }, { "type": "String", "value": type_json }]));
    }
}

/// The `save` input's state, as the driver drives it: the last request it saw and whether it
/// holds `true` (raised, no `saved_map` yet).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SavePulse {
    seen: u64,
    raised: bool,
}

impl SavePulse {
    /// The workload's save counter is `count`. A new request raises `save`: `Some(true)` is the
    /// value to push. A request while one is pending, or no new request, pushes nothing.
    pub fn request(&mut self, count: u64) -> Option<bool> {
        if count <= self.seen {
            return None;
        }
        self.seen = count;
        if self.raised {
            return None;
        }
        self.raised = true;
        Some(true)
    }

    /// The graph gave `saved_map` on a tick (the edge was taken): `Some(false)` is the value to
    /// push to lower `save`, once.
    pub fn saved(&mut self) -> Option<bool> {
        std::mem::take(&mut self.raised).then_some(false)
    }

    /// Whether `save` is raised now.
    #[cfg(test)]
    pub fn raised(&self) -> bool {
        self.raised
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_request_is_one_rising_edge() {
        let mut pulse = SavePulse::default();
        assert_eq!(pulse.request(0), None, "no request yet");
        assert_eq!(pulse.request(1), Some(true), "a new request raises save");
        assert_eq!(pulse.request(1), None, "the same request does not raise it again");
        assert_eq!(pulse.saved(), Some(false), "the saved map lowers it, once");
        assert_eq!(pulse.saved(), None, "a later tick without a save changes nothing");
        assert!(!pulse.raised());
    }

    #[test]
    fn requests_while_one_is_pending_share_its_edge() {
        let mut pulse = SavePulse::default();
        assert_eq!(pulse.request(1), Some(true));
        assert_eq!(pulse.request(2), None, "a second request waits for the first edge");
        assert_eq!(pulse.request(3), None);
        assert_eq!(pulse.saved(), Some(false));
        assert_eq!(pulse.request(3), None, "already answered: nothing new");
    }

    #[test]
    fn a_later_request_after_the_lowering_is_a_new_edge() {
        let mut pulse = SavePulse::default();
        assert_eq!(pulse.request(1), Some(true));
        assert_eq!(pulse.saved(), Some(false));
        assert_eq!(pulse.request(2), Some(true));
        assert_eq!(pulse.saved(), Some(false));
    }

    #[test]
    fn a_save_without_a_request_is_ignored() {
        let mut pulse = SavePulse::default();
        assert_eq!(pulse.saved(), None);
        assert_eq!(pulse.request(1), Some(true));
    }

    #[test]
    fn a_missing_map_file_is_named_in_the_error() {
        let error = load_map(Path::new("/nonexistent/helios/map.eidmap")).expect_err("missing");
        assert!(error.contains("/nonexistent/helios/map.eidmap"), "{error}");
    }

    #[test]
    fn the_map_ports_are_wired_once_into_each_live_map_graph() {
        let goldens = [
            include_str!("../../graphs/live-map.graph.json"),
            include_str!("../../graphs/dense-map.graph.json"),
            include_str!("../../graphs/apriltag-36h11-map.graph.json"),
            include_str!("../../graphs/apriltag-36h11-map-dense.graph.json"),
        ];
        for text in goldens {
            let mut graph: serde_json::Value = serde_json::from_str(text).expect("a golden graph");
            assert!(wire_map_ports(&mut graph));
            let once = graph.clone();
            assert!(wire_map_ports(&mut graph));
            assert_eq!(graph, once, "wiring again changes nothing");
            let nodes = graph["graph"]["nodes"].as_array().expect("nodes");
            let host = nodes.iter().find(|node| node["id"] == HOST_NODE).expect("the host");
            assert!(host["outputs"].as_array().expect("outputs").iter().any(|port| port == "save"));
            assert!(host["inputs"].as_array().expect("inputs").iter().any(|port| port == "saved_map"));
            let edges = graph["graph"]["edges"].as_array().expect("edges");
            assert_eq!(edges.iter().filter(|edge| edge["to"]["port"] == "save").count(), 1, "one save edge");
            assert_eq!(edges.iter().filter(|edge| edge["to"]["port"] == "map").count(), 1, "one map edge");
        }
    }

    #[test]
    fn a_graph_without_a_live_map_is_left_alone() {
        let mut graph = serde_json::json!({ "format": "daedalus.graph", "graph": { "nodes": [{ "id": HOST_NODE, "inputs": [], "outputs": [], "metadata": {} }], "edges": [] } });
        let before = graph.clone();
        assert!(!wire_map_ports(&mut graph));
        assert_eq!(graph, before);
    }

    #[test]
    fn a_map_file_loads_its_bytes() {
        let dir = std::env::temp_dir().join(format!("helios-map-io-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("map.eidmap");
        std::fs::write(&path, b"EIDMAP\0\0").expect("write");
        let map = load_map(&path).expect("load");
        assert_eq!(map.bytes, b"EIDMAP\0\0");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
