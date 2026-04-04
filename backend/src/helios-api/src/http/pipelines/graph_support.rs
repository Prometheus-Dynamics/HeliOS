use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap};

use crate::http::pipelines::types::{PlannerDiagnostic, PlannerDiagnosticSpan, RegistryPortMetadataLookup};
use helios_engine::ipc::NodeRegistrySnapshot;

pub(crate) fn normalize_graph_metadata(graph: &mut JsonValue) {
    let Some(obj) = graph.as_object_mut() else { return };
    let Some(values) = obj.get_mut("metadata") else { return };
    let Some(values_obj) = values.as_object_mut() else { return };

    for (_, value) in values_obj.iter_mut() {
        if let JsonValue::Object(map) = value
            && map.contains_key("type")
            && map.contains_key("value")
        {
            continue;
        }
    }
}

fn edge_signature(edge: &JsonValue) -> Option<String> {
    let from = edge.get("from")?;
    let to = edge.get("to")?;
    let from_node = from.get("node")?;
    let from_port = from.get("port")?;
    let to_node = to.get("node")?;
    let to_port = to.get("port")?;
    let from_node = from_node.as_i64().map(|n| n.to_string()).or_else(|| from_node.as_str().map(|s| s.to_string()))?;
    let to_node = to_node.as_i64().map(|n| n.to_string()).or_else(|| to_node.as_str().map(|s| s.to_string()))?;
    let from_port = from_port.as_str()?;
    let to_port = to_port.as_str()?;
    Some(format!("{from_node}:{from_port}->{to_node}:{to_port}"))
}

pub(crate) fn merge_edge_metadata(raw_graph: &JsonValue, graph_json: &mut JsonValue) {
    let Some(raw_edges) = raw_graph.get("edges").and_then(|edges| edges.as_array()) else {
        return;
    };
    let Some(out_edges) = graph_json.get_mut("edges").and_then(|edges| edges.as_array_mut()) else {
        return;
    };

    let mut raw_meta = HashMap::new();
    let mut raw_meta_by_index = Vec::new();
    for edge in raw_edges {
        let signature = edge_signature(edge);
        let metadata = edge.get("metadata").cloned();
        raw_meta_by_index.push((signature.clone(), metadata.clone()));
        if let (Some(signature), Some(metadata)) = (signature, metadata) {
            raw_meta.insert(signature, metadata);
        }
    }

    let mut applied = 0;
    for (idx, edge) in out_edges.iter_mut().enumerate() {
        let signature = edge_signature(edge);
        let metadata = signature.as_ref().and_then(|sig| raw_meta.get(sig)).cloned();
        let metadata = metadata.or_else(|| raw_meta_by_index.get(idx).and_then(|(_, meta)| meta.as_ref()).cloned());
        let Some(metadata) = metadata else { continue };
        if let JsonValue::Object(out_edge) = edge {
            out_edge.insert("metadata".to_string(), metadata);
            applied += 1;
        }
    }

    if applied == 0 && !raw_meta_by_index.is_empty() && raw_meta.is_empty() {
        for (idx, edge) in out_edges.iter_mut().enumerate() {
            let Some((_, Some(metadata))) = raw_meta_by_index.get(idx) else {
                continue;
            };
            if let JsonValue::Object(out_edge) = edge {
                out_edge.insert("metadata".to_string(), metadata.clone());
            }
        }
    }
}

fn normalize_backend_id(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    match lower.split_once('@') {
        Some((base, _)) => base.to_string(),
        None => lower,
    }
}

pub(crate) fn build_registry_port_metadata_lookup(snapshot: &NodeRegistrySnapshot) -> RegistryPortMetadataLookup {
    let mut out = HashMap::new();
    for node in &snapshot.nodes {
        let key = normalize_backend_id(&node.id);
        let mut meta: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for (k, v) in &node.metadata {
            if k.starts_with("inputs.") || k.starts_with("outputs.") || k == "daedalus.embedded_graph" || k == "daedalus.embedded_host" || k == "daedalus.embedded_group" {
                meta.insert(k.clone(), serde_json::to_value(v.clone()).unwrap_or(serde_json::Value::Null));
            }
        }
        if !meta.is_empty() {
            out.insert(key, meta);
        }
    }
    out
}

pub(crate) fn inject_port_metadata_lookup(graph: &mut serde_json::Value, lookup: &RegistryPortMetadataLookup) {
    let nodes = match graph.get_mut("nodes").and_then(|v| v.as_array_mut()) {
        Some(nodes) => nodes,
        None => return,
    };
    if lookup.is_empty() {
        return;
    }
    for node in nodes {
        let id = node.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let normalized = normalize_backend_id(id);
        let Some(meta) = lookup.get(&normalized) else {
            continue;
        };
        let Some(node_obj) = node.as_object_mut() else {
            continue;
        };
        let metadata = node_obj.entry("metadata").or_insert_with(|| serde_json::json!({}));
        let Some(metadata_obj) = metadata.as_object_mut() else {
            continue;
        };
        for (key, value) in meta {
            metadata_obj.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
}

pub(crate) fn inject_port_metadata(graph: &mut serde_json::Value, registry: &NodeRegistrySnapshot) {
    let lookup = build_registry_port_metadata_lookup(registry);
    inject_port_metadata_lookup(graph, &lookup);
}

pub(crate) fn inject_pipeline_alias_metadata(graph: &mut serde_json::Value, name: Option<&str>) {
    let Some(name) = name.map(str::trim).filter(|v| !v.is_empty()) else {
        return;
    };
    let Some(obj) = graph.as_object_mut() else {
        return;
    };
    let metadata = obj.entry("metadata").or_insert_with(|| serde_json::json!({}));
    let Some(meta_obj) = metadata.as_object_mut() else {
        return;
    };
    meta_obj.insert("helios.pipeline.alias".to_string(), serde_json::Value::String(name.to_string()));
}

pub(super) fn pipeline_graph_alias(graph: &serde_json::Value) -> Option<&str> {
    graph.get("metadata").and_then(|meta| meta.as_object()).and_then(|meta| meta.get("helios.pipeline.alias").and_then(|v| v.as_str())).map(str::trim).filter(|v| !v.is_empty())
}

pub(crate) fn normalize_graph_node_ids(graph: &mut daedalus::planner::Graph) {
    for node in &mut graph.nodes {
        if let Some(normalized) = normalize_node_id(&node.id.0) {
            node.id.0 = normalized;
        }
    }
}

fn normalize_node_id(id: &str) -> Option<String> {
    let mut parts = id.split(':').filter(|part| !part.is_empty());
    let root = parts.next()?;
    let rest: Vec<&str> = parts.filter(|part| *part != root).collect();
    if rest.is_empty() {
        return None;
    }
    let normalized = std::iter::once(root).chain(rest).collect::<Vec<_>>().join(":");
    if normalized == id { None } else { Some(normalized) }
}

pub(super) fn unwrap_pipeline_export_graph(payload: &serde_json::Value) -> Option<serde_json::Value> {
    let obj = payload.as_object()?;
    if let Some(graph) = obj.get("graph").and_then(|g| g.as_object()).cloned() {
        return Some(serde_json::Value::Object(graph));
    }
    let pipeline = obj.get("pipeline")?.as_object()?;
    let graph = pipeline.get("graph")?.as_object()?.clone();
    Some(serde_json::Value::Object(graph))
}

pub(super) fn map_planner_diagnostics(diagnostics: Vec<helios_engine::ipc::PlannerDiagnostic>) -> Vec<PlannerDiagnostic> {
    diagnostics
        .into_iter()
        .map(|diag| PlannerDiagnostic { code: diag.code, message: diag.message, span: PlannerDiagnosticSpan { pass: diag.span.pass, node: diag.span.node, port: diag.span.port } })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::pipeline_graph_alias;
    use serde_json::json;

    #[test]
    fn pipeline_graph_alias_reads_canonical_metadata_key() {
        let graph = json!({
            "metadata": {
                "helios.pipeline.alias": "canonical-name"
            }
        });
        assert_eq!(pipeline_graph_alias(&graph), Some("canonical-name"));
    }

    #[test]
    fn pipeline_graph_alias_ignores_legacy_metadata_key() {
        let graph = json!({
            "metadata": {
                "pipeline_alias": "legacy-name"
            }
        });
        assert_eq!(pipeline_graph_alias(&graph), None);
    }
}
