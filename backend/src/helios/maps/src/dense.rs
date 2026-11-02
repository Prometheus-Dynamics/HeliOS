//! The dense mapper's host ports (`eidos:dense.map`). Its whole-mesh output (`mesh`, wired to the
//! host as `dense_mesh`) is retired: a graph reads the submap chunks instead (`mesh_chunk`, the
//! submaps' poses in `transforms`, and `status`). Eidos's templates still wire the whole mesh, so
//! HeliOS rewires it wherever a graph is built, deployed or loaded ([`wire_dense_chunks`]).

use serde_json::{Value, json};

use crate::{HOST_NODE, push_unique};

/// The dense mapper node (`eidos:dense.map`).
pub const DENSE_NODE: &str = "eidos:dense.map";
/// The whole-mesh host input, retired: no graph wires it any more.
pub const LEGACY_MESH_PORT: &str = "dense_mesh";
/// The dense mapper's outputs the host takes instead: the submap chunks, their poses and the status.
pub const CHUNK_PORTS: [&str; 3] = ["mesh_chunk", "transforms", "status"];
/// The dense mapper's whole-mesh output (the port `LEGACY_MESH_PORT` was wired from).
const MESH_OUTPUT: &str = "mesh";

/// Rewires a dense graph (`GraphDocument` JSON) from the whole mesh to the chunk outputs: the host
/// takes `mesh_chunk`, `transforms` and `status` from the dense mapper, and `dense_mesh` is no
/// longer wired. Idempotent. Returns whether anything changed; `false` when the graph has no dense
/// mapper or no host bridge.
pub fn wire_dense_chunks(graph: &mut Value) -> bool {
    let Some(nodes) = graph.pointer("/graph/nodes").and_then(Value::as_array) else { return false };
    let position = |id: &str| nodes.iter().position(|node| node.get("id").and_then(Value::as_str) == Some(id));
    let (Some(host), Some(dense)) = (position(HOST_NODE), position(DENSE_NODE)) else { return false };
    let mut changed = false;

    let nodes = graph.pointer_mut("/graph/nodes").and_then(Value::as_array_mut).expect("nodes were just read");
    let host_node = &mut nodes[host];
    if let Some(inputs) = host_node["inputs"].as_array_mut() {
        let before = inputs.len();
        inputs.retain(|port| port.as_str() != Some(LEGACY_MESH_PORT));
        changed |= inputs.len() != before;
    }
    for port in CHUNK_PORTS {
        let inputs = &mut host_node["inputs"];
        if !inputs.as_array().is_some_and(|list| list.iter().any(|entry| entry.as_str() == Some(port))) {
            push_unique(inputs, port);
            changed = true;
        }
    }

    let Some(edges) = graph.pointer_mut("/graph/edges").and_then(Value::as_array_mut) else { return changed };
    let legacy = json!({ "node": host, "port": LEGACY_MESH_PORT });
    let before = edges.len();
    edges.retain(|edge| edge["to"] != legacy);
    changed |= edges.len() != before;
    for port in CHUNK_PORTS {
        let from = json!({ "node": dense, "port": port });
        let to = json!({ "node": host, "port": port });
        if !edges.iter().any(|edge| edge["from"] == from && edge["to"] == to) {
            edges.push(json!({ "from": from, "to": to }));
            changed = true;
        }
    }
    // The whole mesh's own edge (dense.mesh -> host) would be left dangling if any other input
    // took it; none does, but it is the output this rewire retires, so it goes with the input.
    let mesh_out = json!({ "node": dense, "port": MESH_OUTPUT });
    let before = edges.len();
    edges.retain(|edge| edge["from"] != mesh_out || edge["to"]["node"] != host);
    changed |= edges.len() != before;
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dense graph as Eidos's template wires it: the whole mesh to the host, timing and depth too.
    fn legacy_graph() -> Value {
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

    #[test]
    fn the_whole_mesh_is_replaced_by_the_chunk_ports() {
        let mut graph = legacy_graph();
        assert!(wire_dense_chunks(&mut graph));
        let host = &graph["graph"]["nodes"][0];
        let inputs: Vec<&str> = host["inputs"].as_array().expect("inputs").iter().filter_map(Value::as_str).collect();
        assert_eq!(inputs, ["frame", "dense_timing", "dense_depth", "mesh_chunk", "transforms", "status"]);
        let edges = graph["graph"]["edges"].as_array().expect("edges");
        assert!(!edges.iter().any(|e| e["to"]["port"] == "dense_mesh" || e["from"]["port"] == "mesh"), "no whole-mesh edge: {edges:?}");
        for port in CHUNK_PORTS {
            assert!(edges.contains(&json!({ "from": { "node": 1, "port": port }, "to": { "node": 0, "port": port } })), "{port} is wired");
        }
        assert_eq!(edges.len(), 2 + 3, "timing and depth kept, the three chunk ports added, the mesh edge gone: {edges:?}");
    }

    #[test]
    fn a_rewired_graph_is_left_alone() {
        let mut graph = legacy_graph();
        assert!(wire_dense_chunks(&mut graph));
        let once = graph.clone();
        assert!(!wire_dense_chunks(&mut graph), "idempotent");
        assert_eq!(graph, once);
    }

    #[test]
    fn a_graph_without_the_dense_mapper_or_a_host_is_not_touched() {
        let mut no_dense = legacy_graph();
        no_dense["graph"]["nodes"][1]["id"] = json!("eidos:mapping.live_map");
        let before = no_dense.clone();
        assert!(!wire_dense_chunks(&mut no_dense));
        assert_eq!(no_dense, before);

        let mut no_host = legacy_graph();
        no_host["graph"]["nodes"][0]["id"] = json!("io.other");
        assert!(!wire_dense_chunks(&mut no_host));
    }
}
