use std::collections::BTreeMap;

use chrono::Utc;
use daedalus::planner::Graph as DaedalusGraph;
use serde_json::Value as JsonValue;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::pipelines::PipelineRefreshFailure;
use crate::http::pipelines::{inject_pipeline_alias_metadata, merge_edge_metadata, normalize_graph_metadata, normalize_graph_node_ids, refresh_graph_validation, refresh_pipeline_input_consumers};

use super::document::{encode_daedalus_value, load_pipeline_doc, save_pipeline_doc};

pub(crate) async fn update_pipeline_node_const(state: &AppState, pipeline_id: Uuid, node_id: &str, port: &str, value: Option<JsonValue>) -> Result<i64, String> {
    let mut doc = load_pipeline_doc(pipeline_id).await?;
    let graph = doc.graph.as_object_mut().ok_or_else(|| "pipeline graph is not an object".to_string())?;
    let nodes = graph.get_mut("nodes").and_then(|v| v.as_array_mut()).ok_or_else(|| "pipeline graph missing nodes".to_string())?;
    let node = nodes.iter_mut().find(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(node_id)).and_then(|entry| entry.as_object_mut()).ok_or_else(|| format!("node {node_id} not found"))?;

    let const_inputs = node.entry("const_inputs").or_insert_with(|| JsonValue::Array(Vec::new()));
    let arr = const_inputs.as_array_mut().ok_or_else(|| "const_inputs is not an array".to_string())?;

    let mut matched = false;
    arr.retain_mut(|entry| match entry {
        JsonValue::Array(items) if items.len() >= 2 => {
            if items[0].as_str() == Some(port) {
                matched = true;
                if let Some(next) = value.as_ref().map(encode_daedalus_value).and_then(|v| serde_json::to_value(v).ok()) {
                    items[1] = next;
                    true
                } else {
                    false
                }
            } else {
                true
            }
        }
        _ => true,
    });

    if !matched && let Some(next) = value.as_ref().map(encode_daedalus_value).and_then(|v| serde_json::to_value(v).ok()) {
        arr.push(JsonValue::Array(vec![JsonValue::String(port.to_string()), next]));
    }

    doc.updated_at_ms = Utc::now().timestamp_millis();
    save_pipeline_doc(pipeline_id, &doc).await?;
    refresh_graph_validation(state, pipeline_id, &doc.graph).await;
    let failures = crate::http::pipelines::refresh_pipeline_consumers(state, pipeline_id, &doc.graph).await;
    if !failures.is_empty() {
        return Err(format_refresh_failures("graph saved but failed to refresh streams", &failures));
    }
    Ok(doc.updated_at_ms)
}

pub(crate) async fn update_pipeline_input_value(state: &AppState, pipeline_id: Uuid, port: &str, value: Option<JsonValue>) -> Result<i64, String> {
    let runtime_value = value.clone();
    let mut doc = load_pipeline_doc(pipeline_id).await?;
    let graph = doc.graph.as_object_mut().ok_or_else(|| "pipeline graph is not an object".to_string())?;
    let entry = graph.entry("pipelineInputValues").or_insert_with(|| JsonValue::Object(serde_json::Map::new()));
    let obj = entry.as_object_mut().ok_or_else(|| "pipelineInputValues is not an object".to_string())?;

    if let Some(raw) = value {
        let encoded = encode_daedalus_value(&raw);
        let json = serde_json::to_value(encoded).map_err(|_| "failed to encode value".to_string())?;
        obj.insert(port.to_string(), json);
    } else {
        obj.remove(port);
    }

    doc.updated_at_ms = Utc::now().timestamp_millis();
    save_pipeline_doc(pipeline_id, &doc).await?;
    let mut inputs = BTreeMap::new();
    inputs.insert(port.to_string(), runtime_value);
    let failures = refresh_pipeline_input_consumers(state, pipeline_id, inputs).await;
    if !failures.is_empty() {
        return Err(format_refresh_failures("inputs saved but failed to refresh streams", &failures));
    }
    Ok(doc.updated_at_ms)
}

pub(crate) async fn update_pipeline_graph(state: &AppState, pipeline_id: Uuid, graph: JsonValue, name: Option<String>) -> Result<i64, String> {
    let mut doc = load_pipeline_doc(pipeline_id).await?;

    let mut raw_graph = graph.clone();
    normalize_graph_metadata(&mut raw_graph);

    let mut daedalus_graph: DaedalusGraph = serde_json::from_value(raw_graph.clone()).map_err(|err| format!("invalid daedalus graph: {err}"))?;
    normalize_graph_node_ids(&mut daedalus_graph);
    let mut graph_json = serde_json::to_value(&daedalus_graph).map_err(|err| format!("failed to encode graph: {err}"))?;

    state.services.pipelines.inject_cached_port_metadata(state, &mut graph_json).await;
    merge_edge_metadata(&graph, &mut graph_json);
    inject_pipeline_alias_metadata(&mut graph_json, name.as_deref());

    doc.graph = graph_json;
    if let Some(name) = name {
        doc.name = Some(name);
    }
    doc.updated_at_ms = Utc::now().timestamp_millis();
    save_pipeline_doc(pipeline_id, &doc).await?;
    let failures = crate::http::pipelines::refresh_pipeline_consumers(state, pipeline_id, &doc.graph).await;
    if !failures.is_empty() {
        return Err(format_refresh_failures("graph saved but failed to refresh streams", &failures));
    }
    Ok(doc.updated_at_ms)
}

fn format_refresh_failures(prefix: &str, failures: &[PipelineRefreshFailure]) -> String {
    let detail = failures.iter().map(|failure| format!("{}: {}", failure.stream_id, failure.error)).collect::<Vec<_>>().join(", ");
    format!("{prefix}: {detail}")
}
