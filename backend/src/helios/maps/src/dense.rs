//! The dense mapper's host ports (`eidos:dense.map`). A graph reads the submap chunks, which Eidos's
//! template wires to the host as `dense_mesh_chunk`, `dense_transforms` and `dense_status` (the
//! `eidos` `ports::DENSE_*` constants). The whole-mesh output is gone from Eidos.
//!
//! Stored graphs from before carry one of two older forms: the whole mesh (`dense_mesh`, wired from
//! the mapper's `mesh`), or HeliOS's own rewire of the chunks (host ports `mesh_chunk`, `transforms`
//! and `status`). [`migrate_dense_ports`] brings either to Eidos's names. Templates need no rewire:
//! they are Eidos's output already.

use serde_json::{Value, json};

use crate::HOST_NODE;

/// The dense mapper node (`eidos:dense.map`).
pub const DENSE_NODE: &str = "eidos:dense.map";
/// The dense mapper's chunk outputs (its node ports), in the order of [`CHUNK_HOST_PORTS`].
pub const CHUNK_OUTPUTS: [&str; 3] = ["mesh_chunk", "transforms", "status"];
/// The host ports those outputs reach, as Eidos's template wires them (`ports::DENSE_MESH_CHUNK`,
/// `ports::DENSE_TRANSFORMS`, `ports::DENSE_STATUS`).
pub const CHUNK_HOST_PORTS: [&str; 3] = ["dense_mesh_chunk", "dense_transforms", "dense_status"];
/// The whole-mesh host port, retired (Eidos dropped it).
const RETIRED_MESH_HOST_PORT: &str = "dense_mesh";
/// The whole-mesh output, retired (Eidos dropped it).
const RETIRED_MESH_OUTPUT: &str = "mesh";

/// Brings a dense graph (`GraphDocument` JSON) to Eidos's host port names. Both older forms end the
/// same way: the host takes `dense_mesh_chunk`, `dense_transforms` and `dense_status` from the
/// mapper, the whole-mesh host port and its edge are gone, and the mapper's `mesh` output is dropped
/// from the node's port list. Other ports and edges are left as they are. Idempotent. Returns whether
/// anything changed; `false` when the graph has no dense mapper or no host bridge.
pub fn migrate_dense_ports(graph: &mut Value) -> bool {
    let original = graph.clone();
    let Some(nodes) = graph.pointer("/graph/nodes").and_then(Value::as_array) else { return false };
    let position = |id: &str| nodes.iter().position(|node| node.get("id").and_then(Value::as_str) == Some(id));
    let (Some(host), Some(dense)) = (position(HOST_NODE), position(DENSE_NODE)) else { return false };

    let nodes = graph.pointer_mut("/graph/nodes").and_then(Value::as_array_mut).expect("nodes were just read");
    if let Some(outputs) = nodes[dense].get_mut("outputs").and_then(Value::as_array_mut) {
        outputs.retain(|port| port.as_str() != Some(RETIRED_MESH_OUTPUT));
    }
    if let Some(inputs) = nodes[host].get_mut("inputs").and_then(Value::as_array_mut) {
        let mut next: Vec<Value> = Vec::with_capacity(inputs.len() + CHUNK_HOST_PORTS.len());
        for port in inputs.iter() {
            if port.as_str() == Some(RETIRED_MESH_HOST_PORT) {
                continue;
            }
            let renamed = port.as_str().and_then(|name| CHUNK_OUTPUTS.iter().position(|output| *output == name)).map_or_else(|| port.clone(), |i| json!(CHUNK_HOST_PORTS[i]));
            if !next.contains(&renamed) {
                next.push(renamed);
            }
        }
        for port in CHUNK_HOST_PORTS {
            if !next.iter().any(|entry| entry.as_str() == Some(port)) {
                next.push(json!(port));
            }
        }
        *inputs = next;
    }

    if let Some(edges) = graph.pointer_mut("/graph/edges").and_then(Value::as_array_mut) {
        let retired_mesh_edge = |edge: &Value| edge["to"]["node"] == json!(host) && edge["to"]["port"] == RETIRED_MESH_HOST_PORT;
        let whole_mesh = |edge: &Value| edge["from"]["node"] == json!(dense) && edge["from"]["port"] == RETIRED_MESH_OUTPUT && edge["to"]["node"] == json!(host);
        edges.retain(|edge| !retired_mesh_edge(edge) && !whole_mesh(edge));
        // HeliOS's rewire: the chunk outputs wired to the host under their own names.
        for edge in edges.iter_mut() {
            if edge["from"]["node"] != json!(dense) || edge["to"]["node"] != json!(host) {
                continue;
            }
            let Some(from) = edge["from"]["port"].as_str() else { continue };
            let renamed = CHUNK_OUTPUTS.iter().position(|output| *output == from).filter(|_| edge["to"]["port"].as_str() == Some(from));
            if let Some(i) = renamed {
                edge["to"]["port"] = json!(CHUNK_HOST_PORTS[i]);
            }
        }
        for (output, port) in CHUNK_OUTPUTS.iter().zip(CHUNK_HOST_PORTS) {
            let from = json!({ "node": dense, "port": output });
            let to = json!({ "node": host, "port": port });
            if !edges.iter().any(|edge| edge["from"] == from && edge["to"] == to) {
                edges.push(json!({ "from": from, "to": to }));
            }
        }
    }
    *graph != original
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stored form before Eidos's chunk ports: the whole mesh to the host, timing and depth too.
    fn whole_mesh_graph() -> Value {
        json!({
            "format": "daedalus.graph",
            "graph": {
                "nodes": [
                    { "id": "io.host_bridge", "inputs": ["frame", "dense_mesh", "dense_timing", "dense_depth"], "outputs": ["frame"] },
                    { "id": "eidos:dense.map", "inputs": ["frame", "pose"], "outputs": ["mesh", "mesh_chunk", "transforms", "status", "timing", "depth"] }
                ],
                "edges": [
                    { "from": { "node": 1, "port": "mesh" }, "to": { "node": 0, "port": "dense_mesh" } },
                    { "from": { "node": 1, "port": "timing" }, "to": { "node": 0, "port": "dense_timing" } },
                    { "from": { "node": 1, "port": "depth" }, "to": { "node": 0, "port": "dense_depth" } }
                ]
            }
        })
    }

    /// The stored form HeliOS wrote before Eidos's names: the chunks on the host as `mesh_chunk`,
    /// `transforms` and `status`.
    fn helios_chunks_graph() -> Value {
        json!({
            "format": "daedalus.graph",
            "graph": {
                "nodes": [
                    { "id": "io.host_bridge", "inputs": ["frame", "dense_timing", "dense_depth", "mesh_chunk", "transforms", "status"], "outputs": ["frame"] },
                    { "id": "eidos:dense.map", "inputs": ["frame", "pose"], "outputs": ["mesh", "mesh_chunk", "transforms", "status", "timing", "depth"] }
                ],
                "edges": [
                    { "from": { "node": 1, "port": "timing" }, "to": { "node": 0, "port": "dense_timing" } },
                    { "from": { "node": 1, "port": "depth" }, "to": { "node": 0, "port": "dense_depth" } },
                    { "from": { "node": 1, "port": "mesh_chunk" }, "to": { "node": 0, "port": "mesh_chunk" } },
                    { "from": { "node": 1, "port": "transforms" }, "to": { "node": 0, "port": "transforms" } },
                    { "from": { "node": 1, "port": "status" }, "to": { "node": 0, "port": "status" } }
                ]
            }
        })
    }

    /// The graph as Eidos's template writes it now.
    fn eidos_graph() -> Value {
        json!({
            "format": "daedalus.graph",
            "graph": {
                "nodes": [
                    { "id": "io.host_bridge", "inputs": ["frame", "dense_timing", "dense_depth", "dense_mesh_chunk", "dense_transforms", "dense_status"], "outputs": ["frame"] },
                    { "id": "eidos:dense.map", "inputs": ["frame", "pose"], "outputs": ["mesh_chunk", "transforms", "status", "timing", "depth"] }
                ],
                "edges": [
                    { "from": { "node": 1, "port": "timing" }, "to": { "node": 0, "port": "dense_timing" } },
                    { "from": { "node": 1, "port": "depth" }, "to": { "node": 0, "port": "dense_depth" } },
                    { "from": { "node": 1, "port": "mesh_chunk" }, "to": { "node": 0, "port": "dense_mesh_chunk" } },
                    { "from": { "node": 1, "port": "transforms" }, "to": { "node": 0, "port": "dense_transforms" } },
                    { "from": { "node": 1, "port": "status" }, "to": { "node": 0, "port": "dense_status" } }
                ]
            }
        })
    }

    /// The migrated graph's host inputs, edges and the dense mapper's outputs must read the same
    /// whichever old form they came from.
    fn assert_eidos_names(graph: &Value) {
        let host = &graph["graph"]["nodes"][0];
        let inputs: Vec<&str> = host["inputs"].as_array().expect("inputs").iter().filter_map(Value::as_str).collect();
        assert_eq!(inputs, ["frame", "dense_timing", "dense_depth", "dense_mesh_chunk", "dense_transforms", "dense_status"]);
        let outputs: Vec<&str> = graph["graph"]["nodes"][1]["outputs"].as_array().expect("outputs").iter().filter_map(Value::as_str).collect();
        assert_eq!(outputs, ["mesh_chunk", "transforms", "status", "timing", "depth"], "the whole mesh is no output");
        let edges = graph["graph"]["edges"].as_array().expect("edges");
        assert_eq!(edges.len(), 2 + 3, "timing and depth kept, the three chunk ports wired: {edges:?}");
        for (output, port) in CHUNK_OUTPUTS.iter().zip(CHUNK_HOST_PORTS) {
            assert!(edges.contains(&json!({ "from": { "node": 1, "port": output }, "to": { "node": 0, "port": port } })), "{port} is wired");
        }
        assert!(!edges.iter().any(|e| e["to"]["port"] == "dense_mesh" || e["from"]["port"] == "mesh" || e["to"]["port"] == "mesh_chunk"), "no whole-mesh or HeliOS edge: {edges:?}");
    }

    #[test]
    fn the_whole_mesh_form_moves_to_the_eidos_chunk_ports() {
        let mut graph = whole_mesh_graph();
        assert!(migrate_dense_ports(&mut graph));
        assert_eidos_names(&graph);
    }

    #[test]
    fn helios_chunk_ports_move_to_the_eidos_names() {
        let mut graph = helios_chunks_graph();
        assert!(migrate_dense_ports(&mut graph));
        assert_eidos_names(&graph);
    }

    #[test]
    fn both_old_forms_migrate_to_the_same_graph() {
        let mut whole = whole_mesh_graph();
        let mut helios = helios_chunks_graph();
        migrate_dense_ports(&mut whole);
        migrate_dense_ports(&mut helios);
        assert_eq!(whole["graph"]["edges"].as_array().map(|e| e.len()), helios["graph"]["edges"].as_array().map(|e| e.len()));
        assert_eq!(whole["graph"]["nodes"][0]["inputs"], helios["graph"]["nodes"][0]["inputs"]);
    }

    #[test]
    fn a_migrated_graph_is_left_alone() {
        for mut graph in [whole_mesh_graph(), helios_chunks_graph()] {
            assert!(migrate_dense_ports(&mut graph));
            let once = graph.clone();
            assert!(!migrate_dense_ports(&mut graph), "idempotent");
            assert_eq!(graph, once);
        }
    }

    #[test]
    fn an_eidos_graph_is_not_changed() {
        let mut graph = eidos_graph();
        let before = graph.clone();
        assert!(!migrate_dense_ports(&mut graph));
        assert_eq!(graph, before);
    }

    #[test]
    fn a_graph_without_the_dense_mapper_or_a_host_is_not_touched() {
        let mut no_dense = whole_mesh_graph();
        no_dense["graph"]["nodes"][1]["id"] = json!("eidos:mapping.live_map");
        let before = no_dense.clone();
        assert!(!migrate_dense_ports(&mut no_dense));
        assert_eq!(no_dense, before);

        let mut no_host = whole_mesh_graph();
        no_host["graph"]["nodes"][0]["id"] = json!("io.other");
        assert!(!migrate_dense_ports(&mut no_host));
    }
}
