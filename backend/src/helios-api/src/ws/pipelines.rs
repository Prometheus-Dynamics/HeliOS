use crate::http::AppState;
use crate::http::pipelines::PipelineDocument;
use crate::http::pipelines::{
    inject_pipeline_alias_metadata, inject_port_metadata, merge_edge_metadata, normalize_graph_metadata, normalize_graph_node_ids, refresh_graph_validation, refresh_pipeline_input_consumers,
};
use crate::http::storage;
use axum::{
    Router,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use chrono::Utc;
use daedalus::data::model::Value as DaedalusValue;
use daedalus::planner::Graph as DaedalusGraph;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio::fs;
use tracing::debug;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new().route("/:id/updates", get(pipeline_updates))
}

async fn pipeline_updates(ws: WebSocketUpgrade, State(state): State<AppState>, Path(id): Path<Uuid>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_pipeline_updates(socket, state, id))
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PipelineUpdateRequest {
    SetNodeConst {
        #[serde(default)]
        request_id: Option<String>,
        node_id: String,
        port: String,
        value: Option<JsonValue>,
    },
    SetInputValue {
        #[serde(default)]
        request_id: Option<String>,
        port: String,
        value: Option<JsonValue>,
    },
    SetGraph {
        #[serde(default)]
        request_id: Option<String>,
        graph: JsonValue,
        #[serde(default)]
        name: Option<String>,
    },
    Ping {
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PipelineUpdateResponse {
    Ack {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        updated_at_ms: Option<i64>,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        error: String,
    },
}

async fn handle_pipeline_updates(mut socket: WebSocket, state: AppState, pipeline_id: Uuid) {
    while let Some(msg) = socket.next().await {
        let payload = match msg {
            Ok(Message::Text(text)) => text,
            Ok(Message::Ping(bytes)) => {
                let _ = socket.send(Message::Pong(bytes)).await;
                continue;
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(err) => {
                debug!(error = %err, "pipeline updates websocket closed");
                break;
            }
        };

        let parsed = serde_json::from_str::<PipelineUpdateRequest>(&payload);
        let response = match parsed {
            Ok(PipelineUpdateRequest::Ping { request_id }) => PipelineUpdateResponse::Ack { request_id, updated_at_ms: None },
            Ok(PipelineUpdateRequest::SetNodeConst { request_id, node_id, port, value }) => match update_pipeline_node_const(&state, pipeline_id, &node_id, &port, value).await {
                Ok(updated_at_ms) => {
                    state.publish_realtime_update(
                        crate::ipc::RealtimeUpdateOrigin::Ws,
                        "pipelines",
                        format!("/v1/ws/pipelines/{pipeline_id}/updates"),
                        Some("set_node_const".to_string()),
                        request_id.clone(),
                    );
                    PipelineUpdateResponse::Ack { request_id, updated_at_ms: Some(updated_at_ms) }
                }
                Err(err) => PipelineUpdateResponse::Error { request_id, error: err },
            },
            Ok(PipelineUpdateRequest::SetInputValue { request_id, port, value }) => match update_pipeline_input_value(&state, pipeline_id, &port, value).await {
                Ok(updated_at_ms) => {
                    state.publish_realtime_update(
                        crate::ipc::RealtimeUpdateOrigin::Ws,
                        "pipelines",
                        format!("/v1/ws/pipelines/{pipeline_id}/updates"),
                        Some("set_input_value".to_string()),
                        request_id.clone(),
                    );
                    PipelineUpdateResponse::Ack { request_id, updated_at_ms: Some(updated_at_ms) }
                }
                Err(err) => PipelineUpdateResponse::Error { request_id, error: err },
            },
            Ok(PipelineUpdateRequest::SetGraph { request_id, graph, name }) => match update_pipeline_graph(&state, pipeline_id, graph, name).await {
                Ok(updated_at_ms) => {
                    state.publish_realtime_update(
                        crate::ipc::RealtimeUpdateOrigin::Ws,
                        "pipelines",
                        format!("/v1/ws/pipelines/{pipeline_id}/updates"),
                        Some("set_graph".to_string()),
                        request_id.clone(),
                    );
                    PipelineUpdateResponse::Ack { request_id, updated_at_ms: Some(updated_at_ms) }
                }
                Err(err) => PipelineUpdateResponse::Error { request_id, error: err },
            },
            Err(err) => PipelineUpdateResponse::Error { request_id: None, error: format!("invalid request: {err}") },
        };

        if let Ok(text) = serde_json::to_string(&response)
            && socket.send(Message::Text(text.into())).await.is_err()
        {
            break;
        }
    }
}

async fn update_pipeline_node_const(state: &AppState, pipeline_id: Uuid, node_id: &str, port: &str, value: Option<JsonValue>) -> Result<i64, String> {
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
        let detail = failures.iter().map(|failure| format!("{}: {}", failure.stream_id, failure.error)).collect::<Vec<_>>().join(", ");
        return Err(format!("graph saved but failed to refresh streams: {detail}"));
    }
    Ok(doc.updated_at_ms)
}

async fn update_pipeline_input_value(state: &AppState, pipeline_id: Uuid, port: &str, value: Option<JsonValue>) -> Result<i64, String> {
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
        let detail = failures.iter().map(|failure| format!("{}: {}", failure.stream_id, failure.error)).collect::<Vec<_>>().join(", ");
        return Err(format!("inputs saved but failed to refresh streams: {detail}"));
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

    if let Ok(snapshot) = state.engine.get_node_registry().await {
        inject_port_metadata(&mut graph_json, &snapshot);
    }
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
        let detail = failures.iter().map(|failure| format!("{}: {}", failure.stream_id, failure.error)).collect::<Vec<_>>().join(", ");
        return Err(format!("graph saved but failed to refresh streams: {detail}"));
    }
    Ok(doc.updated_at_ms)
}

pub(crate) async fn load_pipeline_doc(pipeline_id: Uuid) -> Result<PipelineDocument, String> {
    let path = pipeline_path(pipeline_id).await.map_err(|err| format!("failed to resolve pipeline path: {err}"))?;
    let data = fs::read(&path).await.map_err(|err| format!("failed to read pipeline: {err}"))?;
    serde_json::from_slice::<PipelineDocument>(&data).map_err(|err| format!("failed to decode pipeline: {err}"))
}

pub(crate) async fn save_pipeline_doc(pipeline_id: Uuid, doc: &PipelineDocument) -> Result<(), String> {
    let path = pipeline_path(pipeline_id).await.map_err(|err| format!("failed to resolve pipeline path: {err}"))?;
    let data = serde_json::to_vec_pretty(doc).map_err(|err| format!("failed to encode pipeline: {err}"))?;
    fs::write(&path, data).await.map_err(|err| format!("failed to write pipeline: {err}"))
}

pub(crate) async fn pipeline_path(pipeline_id: Uuid) -> std::io::Result<PathBuf> {
    let dir = storage::ensure_subdir_async("pipelines").await?;
    Ok(dir.join(format!("{pipeline_id}.json")))
}

fn encode_daedalus_value(value: &JsonValue) -> DaedalusValue {
    match value {
        JsonValue::Null => DaedalusValue::Unit,
        JsonValue::Bool(b) => DaedalusValue::Bool(*b),
        JsonValue::Number(num) => {
            if let Some(i) = num.as_i64() {
                DaedalusValue::Int(i)
            } else if let Some(f) = num.as_f64() {
                DaedalusValue::Float(f)
            } else {
                DaedalusValue::Unit
            }
        }
        JsonValue::String(s) => DaedalusValue::String(s.to_string().into()),
        JsonValue::Array(items) => DaedalusValue::List(items.iter().map(encode_daedalus_value).collect()),
        JsonValue::Object(map) => {
            let fields = map
                .iter()
                .filter(|(key, _)| !key.trim().is_empty())
                .map(|(name, value)| daedalus::data::model::StructFieldValue { name: name.to_string(), value: encode_daedalus_value(value) })
                .collect();
            DaedalusValue::Struct(fields)
        }
    }
}
